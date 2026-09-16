//! K8s migration integration tests.

use std::path::PathBuf;
use taba_cli::client::LocalClient;
use taba_cli::commands;
use taba_core::{Unit, UnitKind};
use taba_k8s::K8sConverter;

fn write_toml(dir: &std::path::Path, name: &str, toml: &str) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    std::fs::write(&path, toml).expect("write toml");
    path
}

#[tokio::test]
async fn test_k8s_multi_resource_apply() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    commands::run_init(Some(state.clone()), false).expect("init");

    let yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web-app
spec:
  replicas: 2
  selector:
    matchLabels:
      app: web
  template:
    spec:
      containers:
        - name: web
          image: nginx:1.25
          ports:
            - containerPort: 80
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: web-config
data:
  database_url: postgres://localhost:5432
  log_level: info
---
apiVersion: v1
kind: Service
metadata:
  name: web-service
spec:
  type: LoadBalancer
  ports:
    - port: 80
  selector:
    app: web
";

    let converter = K8sConverter::new();
    let report = converter.convert(yaml).expect("conversion should succeed");

    assert_eq!(report.generated_count(), 3, "should generate 3 units");

    for (name, toml) in &report.generated {
        let file = write_toml(tmp.path(), name, toml);
        commands::run_apply(Some(state.clone()), &file, false)
            .await
            .expect("apply should succeed for each unit");
    }

    let client = LocalClient::load_unverified(Some(state.clone()))
        .await
        .expect("load");
    let units = client.list_units().await.expect("list");
    assert_eq!(
        units
            .iter()
            .filter(|u| !matches!(u, Unit::Governance(_)))
            .count(),
        3,
        "should have 3 non-governance units after migration"
    );

    let kinds: Vec<_> = units.iter().map(taba_core::Unit::kind).collect();
    assert!(
        kinds.contains(&UnitKind::Workload),
        "should contain workload"
    );
    assert!(kinds.contains(&UnitKind::Data), "should contain data");
}

#[tokio::test]
async fn test_k8s_statefulset_recovery() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    commands::run_init(Some(state.clone()), false).expect("init");

    let yaml = r"
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: database
spec:
  replicas: 3
  selector:
    matchLabels:
      app: db
  template:
    spec:
      containers:
        - name: postgres
          image: postgres:16
          ports:
            - containerPort: 5432
  volumeClaimTemplates:
    - metadata:
        name: data
      spec:
        accessModes: ['ReadWriteOnce']
";

    let converter = K8sConverter::new();
    let report = converter.convert(yaml).expect("conversion should succeed");

    let toml = &report.generated["database"];
    assert!(
        toml.contains("require-quorum"),
        "StatefulSet should have recovery"
    );

    let file = write_toml(tmp.path(), "database", toml);
    commands::run_apply(Some(state.clone()), &file, false)
        .await
        .expect("apply");

    let client = LocalClient::load_unverified(Some(state.clone()))
        .await
        .expect("load");
    let units = client.list_units().await.expect("list");
    assert_eq!(
        units
            .iter()
            .filter(|u| !matches!(u, Unit::Governance(_)))
            .count(),
        1
    );

    let workload_unit: Vec<_> = units
        .iter()
        .filter(|u| u.kind() == taba_core::UnitKind::Workload)
        .cloned()
        .collect();
    let taba_core::Unit::Workload(workload) = &workload_unit[0] else {
        panic!("expected workload")
    };
    assert!(workload.artifact.artifact_ref.contains("postgres:16"));
}

#[tokio::test]
async fn test_k8s_daemonset_one_per_node() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    commands::run_init(Some(state.clone()), false).expect("init");

    let yaml = r"
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: log-collector
spec:
  selector:
    matchLabels:
      app: logs
  template:
    spec:
      nodeSelector:
        node-role: worker
      containers:
        - name: fluentd
          image: fluentd:v1.16
";

    let converter = K8sConverter::new();
    let report = converter.convert(yaml).expect("conversion should succeed");

    let toml = &report.generated["log-collector"];
    assert!(
        toml.contains("one instance per node"),
        "DaemonSet should note one-per-node"
    );

    let file = write_toml(tmp.path(), "log-collector", toml);
    commands::run_apply(Some(state.clone()), &file, false)
        .await
        .expect("apply");
}

#[tokio::test]
async fn test_k8s_secret_classification() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    commands::run_init(Some(state.clone()), false).expect("init");

    let yaml = r"
apiVersion: v1
kind: Secret
metadata:
  name: db-credentials
type: Opaque
data:
  password: cGFzc3dvcmQ=
";

    let converter = K8sConverter::new();
    let report = converter.convert(yaml).expect("conversion should succeed");

    let toml = &report.generated["db-credentials"];
    assert!(
        toml.contains("confidential"),
        "Secret should be confidential"
    );
    assert!(
        toml.contains("encrypted_at_rest = true"),
        "Secret should be encrypted"
    );

    eprintln!("DEBUG secret toml: {toml}");
    let file = write_toml(tmp.path(), "db-credentials", toml);
    commands::run_apply(Some(state.clone()), &file, false)
        .await
        .expect("apply");

    let client = LocalClient::load_unverified(Some(state.clone()))
        .await
        .expect("load");
    let units = client.list_units().await.expect("list");
    assert_eq!(
        units
            .iter()
            .filter(|u| !matches!(u, Unit::Governance(_)))
            .count(),
        1
    );
    let data_count = units.iter().filter(|u| u.kind() == UnitKind::Data).count();
    assert_eq!(data_count, 1);
}

#[tokio::test]
async fn test_k8s_report_format() {
    let yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: app
spec:
  selector:
    matchLabels:
      app: app
  template:
    spec:
      containers:
        - name: app
          image: app:v1
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: ingress
";

    let converter = K8sConverter::new();
    let report = converter.convert(yaml).expect("conversion should succeed");

    let formatted = report.format();
    assert!(formatted.contains("Generated: 1 units"));
    assert!(formatted.contains("app"));
    assert!(formatted.contains("Unmappable: 1 resources"));
    assert!(formatted.contains("Ingress/ingress"));
}

#[tokio::test]
async fn test_k8s_with_provenance() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    commands::run_init(Some(state.clone()), false).expect("init");

    let yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: prod-app
spec:
  selector:
    matchLabels:
      app: prod
  template:
    spec:
      containers:
        - name: app
          image: app:v1
";

    let converter = K8sConverter::new().with_provenance();
    let report = converter.convert(yaml).expect("conversion should succeed");

    let toml = &report.generated["prod-app"];
    assert!(
        toml.contains("provenance"),
        "should contain provenance placeholder"
    );
    assert!(toml.contains("slsa_level"), "should contain SLSA level");

    let file = write_toml(tmp.path(), "prod-app", toml);
    commands::run_apply(Some(state.clone()), &file, false)
        .await
        .expect("apply with provenance");
}
