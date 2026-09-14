//! K8s-to-taba converter.
//!
//! Reads K8s manifests and generates taba unit declarations (TOML).
//! Surfaces constructs that cannot be mapped via [`ConversionReport`].

#![allow(
    clippy::format_push_string,
    clippy::unused_self,
    clippy::used_underscore_binding
)]

use crate::error::K8sConvertError;
use crate::k8s_types::{
    Container, DaemonSetSpec, DeploymentSpec, K8sManifest, PodSpec, Probe, ServiceSpec,
    StatefulSetSpec,
};
use crate::report::{ConversionReport, UnmappableResource};

/// Result of a conversion: the report and any error.
pub type ConvertResult = Result<ConversionReport, K8sConvertError>;

/// Converts K8s manifests to taba unit declarations.
#[derive(Debug, Clone, Default)]
pub struct K8sConverter {
    trust_domain: Option<String>,
    generate_provenance: bool,
}

impl K8sConverter {
    /// Creates a new converter with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the trust domain for generated units.
    #[must_use]
    pub fn with_trust_domain(mut self, td: &str) -> Self {
        self.trust_domain = Some(td.to_string());
        self
    }

    /// Enables SLSA provenance placeholder generation.
    #[must_use]
    pub const fn with_provenance(mut self) -> Self {
        self.generate_provenance = true;
        self
    }

    /// Converts one or more K8s manifests (YAML, `---` separated).
    ///
    /// # Errors
    ///
    /// - [`K8sConvertError::YamlParse`] if the YAML is invalid.
    pub fn convert(&self, yaml: &str) -> ConvertResult {
        let mut report = ConversionReport::new();

        let docs: Vec<&str> = yaml
            .split("\n---")
            .filter(|d| !d.trim().is_empty())
            .collect();

        for doc in &docs {
            match serde_yaml::from_str::<serde_yaml::Value>(doc) {
                Ok(value) => self.convert_value(&value, &mut report),
                Err(e) => {
                    report
                        .warnings
                        .push(format!("Skipping unparseable document: {e}"));
                }
            }
        }

        Ok(report)
    }

    fn convert_value(&self, value: &serde_yaml::Value, report: &mut ConversionReport) {
        let manifest: K8sManifest = match serde_yaml::from_value(value.clone()) {
            Ok(m) => m,
            Err(e) => {
                report
                    .warnings
                    .push(format!("Skipping unparseable manifest: {e}"));
                return;
            }
        };

        match manifest.kind.as_str() {
            "Deployment" => self.convert_deployment(&manifest, report),
            "StatefulSet" => self.convert_statefulset(&manifest, report),
            "DaemonSet" => self.convert_daemonset(&manifest, report),
            "Pod" => self.convert_pod(&manifest, report),
            "Service" => self.convert_service(&manifest, report),
            "ConfigMap" => self.convert_configmap(&manifest, report),
            "Secret" => self.convert_secret(&manifest, report),
            "PersistentVolume" | "PersistentVolumeClaim" => {
                report.unmappable.push(UnmappableResource {
                    kind: manifest.kind.clone(),
                    name: manifest.metadata.name.clone(),
                    reason: "K8s storage resources do not have direct taba equivalents."
                        .to_string(),
                    suggestion: "Use a data unit with storage_requirements instead.".to_string(),
                });
            }
            "Ingress" => {
                report.unmappable.push(UnmappableResource {
                    kind: "Ingress".to_string(),
                    name: manifest.metadata.name.clone(),
                    reason: "taba has no ingress concept.".to_string(),
                    suggestion: "Map the service backend directly.".to_string(),
                });
            }
            "HorizontalPodAutoscaler" => self.convert_hpa(&manifest, report),
            "Job" | "CronJob" => self.convert_job(&manifest, report),
            "Namespace" | "ServiceAccount" | "Role" | "RoleBinding" | "ClusterRole"
            | "ClusterRoleBinding" => {
                report.skipped.push(format!(
                    "{}/{} (no taba equivalent needed)",
                    manifest.kind, manifest.metadata.name
                ));
            }
            kind if kind.contains("CustomResourceDefinition") || self.is_crd(kind) => {
                report.unmappable.push(UnmappableResource {
                    kind: manifest.kind.clone(),
                    name: manifest.metadata.name.clone(),
                    reason: "Custom Resource Definitions may not map to taba.".to_string(),
                    suggestion: "Write a custom taba unit declaration manually.".to_string(),
                });
            }
            other => {
                report.unmappable.push(UnmappableResource {
                    kind: other.to_string(),
                    name: manifest.metadata.name.clone(),
                    reason: "Unknown or unsupported K8s resource kind.".to_string(),
                    suggestion: "Write a custom taba unit declaration manually.".to_string(),
                });
            }
        }
    }

    fn is_crd(&self, kind: &str) -> bool {
        kind.contains('.') && !kind.starts_with('v')
    }

    // --- Deployment ---

    fn convert_deployment(&self, manifest: &K8sManifest, report: &mut ConversionReport) {
        let spec: DeploymentSpec = match serde_yaml::from_value(manifest.spec.clone()) {
            Ok(s) => s,
            Err(e) => {
                report.warnings.push(format!(
                    "Deployment {}: failed to parse spec: {e}",
                    manifest.metadata.name
                ));
                return;
            }
        };

        let Some(container) = spec.template.spec.containers.first() else {
            report.warnings.push(format!(
                "Deployment {}: no containers, skipping",
                manifest.metadata.name
            ));
            return;
        };

        let Some(image) = &container.image else {
            report.warnings.push(format!(
                "Deployment {}: container {} has no image",
                manifest.metadata.name, container.name
            ));
            return;
        };

        let replicas = spec.replicas.unwrap_or(1);
        let mut toml = format!(
            "[unit]\nname = \"{name}\"\nimage = \"{image}\"\nkind = \"service\"\n\n[scaling]\nmin = {min}\nmax = {max}\n",
            name = manifest.metadata.name,
            image = image,
            min = replicas,
            max = replicas.max(replicas * 3)
        );

        let needs = self.extract_needs(&spec.template.spec);
        if !needs.is_empty() {
            toml.push_str("\n[needs]\n");
            for (cap_name, cap_type, purpose) in &needs {
                match purpose {
                    Some(p) => toml.push_str(&format!(
                        "{cap_name} = {{ type = \"{cap_type}\", purpose = \"{p}\" }}\n"
                    )),
                    None => toml.push_str(&format!("{cap_name} = {{ type = \"{cap_type}\" }}\n")),
                }
            }
        }

        let provides = self.extract_provides(container);
        if !provides.is_empty() {
            toml.push_str("\n[provides]\n");
            for (cap_name, cap_type, purpose) in &provides {
                match purpose {
                    Some(p) => toml.push_str(&format!(
                        "{cap_name} = {{ type = \"{cap_type}\", purpose = \"{p}\" }}\n"
                    )),
                    None => toml.push_str(&format!("{cap_name} = {{ type = \"{cap_type}\" }}\n")),
                }
            }
        }

        if let Some(probe) = &container.liveness_probe {
            if let Some(health_toml) = self.convert_probe(probe) {
                toml.push_str(&health_toml);
            }
        }

        if !container.resources.limits.is_empty() {
            toml.push_str("\n[tolerates]\n");
            for key in container.resources.limits.keys() {
                toml.push_str(&format!("# {key} limit declared\n"));
            }
            toml.push_str("consistency = \"eventual\"\n");
        }

        if self.generate_provenance {
            toml.push_str(
                "\n# SLSA provenance placeholder\n# [provenance]\n# builder = \"\"\n# source_repo = \"\"\n# source_digest = \"\"\n# slsa_level = 1\n",
            );
        }

        report
            .generated
            .insert(manifest.metadata.name.clone(), toml);
    }

    // --- StatefulSet ---

    fn convert_statefulset(&self, manifest: &K8sManifest, report: &mut ConversionReport) {
        let spec: StatefulSetSpec = match serde_yaml::from_value(manifest.spec.clone()) {
            Ok(s) => s,
            Err(e) => {
                report.warnings.push(format!(
                    "StatefulSet {}: failed to parse spec: {e}",
                    manifest.metadata.name
                ));
                return;
            }
        };

        let Some(container) = spec.template.spec.containers.first() else {
            report.warnings.push(format!(
                "StatefulSet {}: no containers, skipping",
                manifest.metadata.name
            ));
            return;
        };

        let image = container.image.clone().unwrap_or_default();
        let replicas = spec.replicas.unwrap_or(1);
        let mut toml = format!(
            "[unit]\nname = \"{name}\"\nimage = \"{image}\"\nkind = \"service\"\n\n[recovery]\nstrategy = \"require-quorum\"\nmin_peers = 1\n\n[scaling]\nmin = {min}\nmax = {max}\n",
            name = manifest.metadata.name,
            image = image,
            min = replicas,
            max = replicas.max(replicas * 3)
        );

        for pvc in &spec.volume_claim_templates {
            toml.push_str(&format!(
                "\n# PVC {} -> data dependency (add to [needs] manually)\n",
                pvc.metadata.name
            ));
        }

        let provides = self.extract_provides(container);
        if !provides.is_empty() {
            toml.push_str("\n[provides]\n");
            for (cap_name, cap_type, purpose) in &provides {
                match purpose {
                    Some(p) => toml.push_str(&format!(
                        "{cap_name} = {{ type = \"{cap_type}\", purpose = \"{p}\" }}\n"
                    )),
                    None => toml.push_str(&format!("{cap_name} = {{ type = \"{cap_type}\" }}\n")),
                }
            }
        }

        report
            .generated
            .insert(manifest.metadata.name.clone(), toml);
    }

    // --- DaemonSet ---

    fn convert_daemonset(&self, manifest: &K8sManifest, report: &mut ConversionReport) {
        let spec: DaemonSetSpec = match serde_yaml::from_value(manifest.spec.clone()) {
            Ok(s) => s,
            Err(e) => {
                report.warnings.push(format!(
                    "DaemonSet {}: failed to parse spec: {e}",
                    manifest.metadata.name
                ));
                return;
            }
        };

        let Some(container) = spec.template.spec.containers.first() else {
            report.warnings.push(format!(
                "DaemonSet {}: no containers, skipping",
                manifest.metadata.name
            ));
            return;
        };

        let image = container.image.clone().unwrap_or_default();
        let mut toml = format!(
            "[unit]\nname = \"{name}\"\nimage = \"{image}\"\nkind = \"service\"\n\n[scaling]\nmin = 1\nmax = 1000\n# DaemonSet: one instance per node\n",
            name = manifest.metadata.name,
            image = image
        );

        if !spec.template.spec.node_selector.is_empty() {
            toml.push_str("\n# Node selector (add as custom tags):\n");
            for (key, value) in &spec.template.spec.node_selector {
                toml.push_str(&format!("# {key} = \"{value}\"\n"));
            }
        }

        let provides = self.extract_provides(container);
        if !provides.is_empty() {
            toml.push_str("\n[provides]\n");
            for (cap_name, cap_type, purpose) in &provides {
                match purpose {
                    Some(p) => toml.push_str(&format!(
                        "{cap_name} = {{ type = \"{cap_type}\", purpose = \"{p}\" }}\n"
                    )),
                    None => toml.push_str(&format!("{cap_name} = {{ type = \"{cap_type}\" }}\n")),
                }
            }
        }

        report
            .generated
            .insert(manifest.metadata.name.clone(), toml);
    }

    // --- Pod ---

    fn convert_pod(&self, manifest: &K8sManifest, report: &mut ConversionReport) {
        let spec: PodSpec = match serde_yaml::from_value(manifest.spec.clone()) {
            Ok(s) => s,
            Err(e) => {
                report.warnings.push(format!(
                    "Pod {}: failed to parse spec: {e}",
                    manifest.metadata.name
                ));
                return;
            }
        };

        let Some(container) = spec.containers.first() else {
            report.warnings.push(format!(
                "Pod {}: no containers, skipping",
                manifest.metadata.name
            ));
            return;
        };

        let image = container.image.clone().unwrap_or_default();
        let toml = format!(
            "[unit]\nname = \"{name}\"\nimage = \"{image}\"\nkind = \"service\"\n\n[scaling]\nmin = 1\nmax = 1\n# Pod without controller\n",
            name = manifest.metadata.name,
            image = image
        );

        report
            .generated
            .insert(manifest.metadata.name.clone(), toml);
    }

    // --- Service ---

    fn convert_service(&self, manifest: &K8sManifest, report: &mut ConversionReport) {
        let spec: ServiceSpec = match serde_yaml::from_value(manifest.spec.clone()) {
            Ok(s) => s,
            Err(e) => {
                report.warnings.push(format!(
                    "Service {}: failed to parse spec: {e}",
                    manifest.metadata.name
                ));
                return;
            }
        };

        if spec.ports.is_empty() {
            report
                .skipped
                .push(format!("Service/{} (no ports)", manifest.metadata.name));
            return;
        }

        let port = spec.ports[0].port;
        let svc_type = spec.type_.as_deref().unwrap_or("ClusterIP");

        let toml = format!(
            "[unit]\nname = \"{name}\"\nimage = \"nginx:alpine\"\nkind = \"service\"\n\n[provides]\n{svc_name} = {{ type = \"network\", purpose = \"{svc_type}\" }}\n\n# K8s Service port: {port}\n\n[scaling]\nmin = 1\nmax = 1\n",
            name = manifest.metadata.name,
            svc_name = manifest.metadata.name,
            svc_type = svc_type,
            port = port,
        );

        report
            .generated
            .insert(manifest.metadata.name.clone(), toml);
    }

    // --- ConfigMap ---

    fn convert_configmap(&self, manifest: &K8sManifest, report: &mut ConversionReport) {
        // ConfigMaps store data at the top level, not in spec.
        if manifest.data.is_null() {
            report
                .skipped
                .push(format!("ConfigMap/{} (empty)", manifest.metadata.name));
            return;
        }

        let data: std::collections::BTreeMap<String, String> =
            match serde_yaml::from_value(manifest.data.clone()) {
                Ok(d) => d,
                Err(e) => {
                    report.warnings.push(format!(
                        "ConfigMap {}: failed to parse data: {e}",
                        manifest.metadata.name
                    ));
                    return;
                }
            };

        if data.is_empty() {
            report
                .skipped
                .push(format!("ConfigMap/{} (empty)", manifest.metadata.name));
            return;
        }

        let key_count = data.len();
        let toml = format!(
            "[unit]\nname = \"{name}\"\ntype = \"data\"\n\n[schema]\nformat = \"text/plain\"\ndefinition = \"configmap with {key_count} keys\"\n\n[classification]\nlevel = \"internal\"\n\n[retention]\nmode = \"persistent\"\nlegal_basis = \"configuration data\"\n\n[provides]\nconfig = {{ type = \"configuration\" }}\n",
            name = manifest.metadata.name,
            key_count = key_count,
        );

        report
            .generated
            .insert(manifest.metadata.name.clone(), toml);
    }

    // --- Secret ---

    fn convert_secret(&self, manifest: &K8sManifest, report: &mut ConversionReport) {
        let toml = format!(
            "[unit]\nname = \"{name}\"\ntype = \"data\"\n\n[schema]\nformat = \"opaque\"\ndefinition = \"k8s secret\"\n\n[classification]\nlevel = \"confidential\"\n\n[retention]\nmode = \"persistent\"\nlegal_basis = \"secret material\"\nmandatory = true\n\n[storage]\nencrypted_at_rest = true\n\n[provides]\nsecret = {{ type = \"secret\" }}\n",
            name = manifest.metadata.name,
        );

        report
            .generated
            .insert(manifest.metadata.name.clone(), toml);
    }

    // --- HPA ---

    fn convert_hpa(&self, manifest: &K8sManifest, report: &mut ConversionReport) {
        report.warnings.push(format!(
            "HorizontalPodAutoscaler {}: convert [scaling] manually.",
            manifest.metadata.name
        ));
        report.skipped.push(format!(
            "HorizontalPodAutoscaler/{} (manual)",
            manifest.metadata.name
        ));
    }

    // --- Job/CronJob ---

    fn convert_job(&self, manifest: &K8sManifest, report: &mut ConversionReport) {
        report.warnings.push(format!(
            "{} {}: convert to bounded-task manually.",
            manifest.kind, manifest.metadata.name
        ));
        report.skipped.push(format!(
            "{}/{} (manual)",
            manifest.kind, manifest.metadata.name
        ));
    }

    // --- Helpers ---

    fn extract_needs(&self, spec: &PodSpec) -> Vec<(String, String, Option<String>)> {
        let mut needs = Vec::new();
        for volume in &spec.volumes {
            if let Some(cm) = &volume.config_map {
                needs.push((cm.name.clone(), "configuration".to_string(), None));
            }
            if let Some(secret) = &volume.secret {
                needs.push((secret.secret_name.clone(), "secret".to_string(), None));
            }
        }
        needs
    }

    fn extract_provides(&self, container: &Container) -> Vec<(String, String, Option<String>)> {
        let mut provides = Vec::new();
        for port in &container.ports {
            let protocol = port.protocol.as_deref().unwrap_or("TCP");
            let purpose = if protocol == "TCP" {
                Some("serving".to_string())
            } else {
                Some(protocol.to_lowercase())
            };
            provides.push((
                format!("port-{}", port.container_port),
                "network".to_string(),
                purpose,
            ));
        }
        if provides.is_empty() && container.image.is_some() {
            provides.push(("compute".to_string(), "compute".to_string(), None));
        }
        provides
    }

    fn convert_probe(&self, probe: &Probe) -> Option<String> {
        if let Some(http) = &probe.http_get {
            let path = http.path.as_deref().unwrap_or("/");
            let port = port_value(&http.port);
            return Some(format!(
                "\n[health]\ntype = \"http\"\npath = \"{path}\"\nport = {port}\ninterval = \"{interval}s\"\ntimeout = \"{timeout}s\"\n",
                path = path,
                port = port,
                interval = probe.period_seconds.unwrap_or(10),
                timeout = probe.timeout_seconds.unwrap_or(2),
            ));
        }
        if let Some(tcp) = &probe.tcp_socket {
            let port = port_value(&tcp.port);
            return Some(format!(
                "\n[health]\ntype = \"tcp\"\nport = {port}\ninterval = \"{interval}s\"\ntimeout = \"{timeout}s\"\n",
                port = port,
                interval = probe.period_seconds.unwrap_or(10),
                timeout = probe.timeout_seconds.unwrap_or(2),
            ));
        }
        if let Some(exec) = &probe.exec {
            if !exec.command.is_empty() {
                let cmd = exec.command.join(" ");
                return Some(format!(
                    "\n[health]\ntype = \"command\"\ncommand = \"{cmd}\"\ninterval = \"{interval}s\"\ntimeout = \"{timeout}s\"\n",
                    cmd = cmd,
                    interval = probe.period_seconds.unwrap_or(10),
                    timeout = probe.timeout_seconds.unwrap_or(2),
                ));
            }
        }
        None
    }
}

/// Extracts a port number from a [`serde_yaml::Value`] (int or string).
fn port_value(v: &serde_yaml::Value) -> u16 {
    v.as_u64()
        .and_then(|n| u16::try_from(n).ok())
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEPLOYMENT_YAML: &str = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web-server
spec:
  replicas: 3
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
          livenessProbe:
            httpGet:
              path: /healthz
              port: 80
            periodSeconds: 10
            timeoutSeconds: 2
";

    #[test]
    fn test_convert_deployment() {
        let converter = K8sConverter::new();
        let report = converter
            .convert(DEPLOYMENT_YAML)
            .expect("conversion should succeed");
        assert_eq!(report.generated_count(), 1);
        assert!(report.generated.contains_key("web-server"));
        let toml = &report.generated["web-server"];
        assert!(toml.contains("nginx:1.25"));
        assert!(toml.contains("kind = \"service\""));
        assert!(toml.contains("min = 3"));
        assert!(toml.contains("[health]"));
        assert!(toml.contains("type = \"http\""));
    }

    const STATEFULSET_YAML: &str = r"
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

    #[test]
    fn test_convert_statefulset() {
        let converter = K8sConverter::new();
        let report = converter
            .convert(STATEFULSET_YAML)
            .expect("conversion should succeed");
        assert_eq!(report.generated_count(), 1);
        let toml = &report.generated["database"];
        assert!(toml.contains("postgres:16"));
        assert!(toml.contains("require-quorum"));
        assert!(toml.contains("PVC data"));
    }

    const DAEMONSET_YAML: &str = r"
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
          ports:
            - containerPort: 24224
";

    #[test]
    fn test_convert_daemonset() {
        let converter = K8sConverter::new();
        let report = converter
            .convert(DAEMONSET_YAML)
            .expect("conversion should succeed");
        assert_eq!(report.generated_count(), 1);
        let toml = &report.generated["log-collector"];
        assert!(toml.contains("fluentd:v1.16"));
        assert!(toml.contains("one instance per node"));
        assert!(toml.contains("node-role"));
    }

    const SERVICE_YAML: &str = r"
apiVersion: v1
kind: Service
metadata:
  name: web-service
spec:
  type: LoadBalancer
  ports:
    - port: 80
      targetPort: 8080
  selector:
    app: web
";

    #[test]
    fn test_convert_service() {
        let converter = K8sConverter::new();
        let report = converter
            .convert(SERVICE_YAML)
            .expect("conversion should succeed");
        assert_eq!(report.generated_count(), 1);
        let toml = &report.generated["web-service"];
        assert!(toml.contains("type = \"network\""));
        assert!(toml.contains("LoadBalancer"));
    }

    const CONFIGMAP_YAML: &str = r"
apiVersion: v1
kind: ConfigMap
metadata:
  name: app-config
data:
  database_url: postgres://localhost:5432
  log_level: info
";

    #[test]
    fn test_convert_configmap() {
        let converter = K8sConverter::new();
        let report = converter
            .convert(CONFIGMAP_YAML)
            .expect("conversion should succeed");
        assert_eq!(report.generated_count(), 1);
        let toml = &report.generated["app-config"];
        assert!(toml.contains("type = \"data\""));
        assert!(toml.contains("internal"));
    }

    const SECRET_YAML: &str = r"
apiVersion: v1
kind: Secret
metadata:
  name: db-credentials
type: Opaque
data:
  password: cGFzc3dvcmQ=
";

    #[test]
    fn test_convert_secret() {
        let converter = K8sConverter::new();
        let report = converter
            .convert(SECRET_YAML)
            .expect("conversion should succeed");
        assert_eq!(report.generated_count(), 1);
        let toml = &report.generated["db-credentials"];
        assert!(toml.contains("type = \"data\""));
        assert!(toml.contains("confidential"));
        assert!(toml.contains("encrypted_at_rest = true"));
    }

    const POD_YAML: &str = r"
apiVersion: v1
kind: Pod
metadata:
  name: debug-pod
spec:
  containers:
    - name: debug
      image: busybox:latest
";

    #[test]
    fn test_convert_pod() {
        let converter = K8sConverter::new();
        let report = converter
            .convert(POD_YAML)
            .expect("conversion should succeed");
        assert_eq!(report.generated_count(), 1);
        assert!(report.generated.contains_key("debug-pod"));
    }

    const INGRESS_YAML: &str = r"
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: web-ingress
spec:
  rules:
    - host: example.com
";

    #[test]
    fn test_convert_ingress_unmappable() {
        let converter = K8sConverter::new();
        let report = converter
            .convert(INGRESS_YAML)
            .expect("conversion should succeed");
        assert_eq!(report.unmappable_count(), 1);
        assert_eq!(report.unmappable[0].kind, "Ingress");
    }

    #[test]
    fn test_convert_pv_unmappable() {
        let yaml = r"
apiVersion: v1
kind: PersistentVolume
metadata:
  name: pv-1
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        assert_eq!(report.unmappable_count(), 1);
    }

    #[test]
    fn test_convert_crd_unmappable() {
        let yaml = r"
apiVersion: example.com/v1
kind: MyCustomResource
metadata:
  name: custom-1
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        assert_eq!(report.unmappable_count(), 1);
    }

    #[test]
    fn test_convert_namespace_skipped() {
        let yaml = r"
apiVersion: v1
kind: Namespace
metadata:
  name: production
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        assert!(report.generated.is_empty());
        assert_eq!(report.skipped.len(), 1);
    }

    #[test]
    fn test_convert_multi_document() {
        let yaml = r"
apiVersion: v1
kind: ConfigMap
metadata:
  name: config-1
data:
  key: value
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: app-1
spec:
  selector:
    matchLabels:
      app: app-1
  template:
    spec:
      containers:
        - name: app
          image: app:v1
          ports:
            - containerPort: 8080
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        assert_eq!(report.generated_count(), 2);
        assert!(report.generated.contains_key("config-1"));
        assert!(report.generated.contains_key("app-1"));
    }

    #[test]
    fn test_convert_hpa_warning() {
        let yaml = r"
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: app-hpa
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn test_convert_job_warning() {
        let yaml = r"
apiVersion: batch/v1
kind: Job
metadata:
  name: data-migration
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn test_convert_empty_configmap_skipped() {
        let yaml = r"
apiVersion: v1
kind: ConfigMap
metadata:
  name: empty-config
data: {}
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        assert!(report.generated.is_empty());
        assert!(!report.skipped.is_empty());
    }

    #[test]
    fn test_convert_deployment_no_containers() {
        let yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: empty-deploy
spec:
  selector:
    matchLabels:
      app: empty
  template:
    spec:
      containers: []
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        assert!(report.generated.is_empty());
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn test_convert_deployment_with_configmap_volume() {
        let yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: app-with-config
spec:
  selector:
    matchLabels:
      app: app
  template:
    spec:
      containers:
        - name: app
          image: app:v1
          ports:
            - containerPort: 80
      volumes:
        - name: config
          configMap:
            name: app-settings
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        let toml = &report.generated["app-with-config"];
        assert!(toml.contains("[needs]"));
        assert!(toml.contains("app-settings"));
        assert!(toml.contains("configuration"));
    }

    #[test]
    fn test_convert_deployment_with_tcp_probe() {
        let yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: tcp-app
spec:
  selector:
    matchLabels:
      app: tcp
  template:
    spec:
      containers:
        - name: app
          image: app:v1
          ports:
            - containerPort: 5432
          livenessProbe:
            tcpSocket:
              port: 5432
            periodSeconds: 5
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        let toml = &report.generated["tcp-app"];
        assert!(toml.contains("type = \"tcp\""));
        assert!(toml.contains("5s"));
    }

    #[test]
    fn test_convert_deployment_with_exec_probe() {
        let yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: exec-app
spec:
  selector:
    matchLabels:
      app: exec
  template:
    spec:
      containers:
        - name: app
          image: app:v1
          livenessProbe:
            exec:
              command:
                - /bin/healthcheck
            periodSeconds: 15
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        let toml = &report.generated["exec-app"];
        assert!(toml.contains("type = \"command\""));
        assert!(toml.contains("/bin/healthcheck"));
    }

    #[test]
    fn test_convert_deployment_with_resource_limits() {
        let yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: resourced-app
spec:
  selector:
    matchLabels:
      app: resourced
  template:
    spec:
      containers:
        - name: app
          image: app:v1
          resources:
            limits:
              memory: 512Mi
              cpu: 1000m
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        let toml = &report.generated["resourced-app"];
        assert!(toml.contains("[tolerates]"));
    }

    #[test]
    fn test_convert_with_provenance() {
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
        assert!(toml.contains("provenance"));
        assert!(toml.contains("slsa_level"));
    }

    #[test]
    fn test_conversion_report_format() {
        let mut report = ConversionReport::new();
        report
            .generated
            .insert("unit-1".to_string(), "toml".to_string());
        report.unmappable.push(UnmappableResource {
            kind: "Ingress".to_string(),
            name: "web".to_string(),
            reason: "no taba equivalent".to_string(),
            suggestion: "use service directly".to_string(),
        });
        report.warnings.push("test warning".to_string());

        let formatted = report.format();
        assert!(formatted.contains("Generated: 1 units"));
        assert!(formatted.contains("unit-1"));
        assert!(formatted.contains("Unmappable: 1 resources"));
        assert!(formatted.contains("Ingress/web"));
        assert!(formatted.contains("Warnings: 1"));
        assert!(!report.is_clean());
    }

    #[test]
    fn test_conversion_report_clean() {
        let mut report = ConversionReport::new();
        report
            .generated
            .insert("unit-1".to_string(), "toml".to_string());
        assert!(report.is_clean());
        assert_eq!(report.total_processed(), 1);
    }

    #[test]
    fn test_convert_invalid_yaml() {
        let yaml = "this is not: valid: yaml: at: all";
        let converter = K8sConverter::new();
        let report = converter
            .convert(yaml)
            .expect("conversion should not error");
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn test_convert_deployment_no_image() {
        let yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: no-image
spec:
  selector:
    matchLabels:
      app: none
  template:
    spec:
      containers:
        - name: app
";
        let converter = K8sConverter::new();
        let report = converter.convert(yaml).expect("conversion should succeed");
        assert!(report.generated.is_empty());
        assert!(!report.warnings.is_empty());
    }
}
