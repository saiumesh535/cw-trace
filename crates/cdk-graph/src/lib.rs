//! Normalized resource graph built from CDK synthesis output (`cdk.out/`).
//!
//! The graph maps CDK construct paths (e.g. `OrdersStack/OrdersApi/CreateOrder`)
//! to the AWS resources they produce and the CloudWatch log groups those
//! resources write to — so the debugger never has to guess a log group name.
//!
//! Inputs (all inside `cdk.out/`):
//! - `tree.json` — construct tree with `aws:cdk:cloudformation:type` / `props`
//! - `manifest.json` — stack → template file + `aws:cdk:logicalId` metadata
//! - `*.template.json` — synthesized CloudFormation with resolved values
//!
//! See `docs/PRD.md` for the product context.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("cdk.out not found at {path}")]
    MissingDir { path: String },
    #[error("io error reading {file}: {source}")]
    Io {
        file: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid {file}: {reason}")]
    InvalidJson { file: String, reason: String },
}

/// An AWS resource kind relevant to log/trace debugging and the construct map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceKind {
    LambdaFunction,
    LogGroup,
    RestApi,
    HttpApi,
    ApiStage,
    EcsService,
    EcsTaskDefinition,
    StateMachine,
    DynamoDbTable,
    SqsQueue,
    SnsTopic,
    S3Bucket,
    KinesisStream,
    EventBridgeRule,
    Unknown,
}

impl ResourceKind {
    /// Map a CloudFormation resource type to a [`ResourceKind`].
    pub fn from_cfn_type(cfn_type: &str) -> Self {
        match cfn_type {
            "AWS::Lambda::Function" => Self::LambdaFunction,
            "AWS::Logs::LogGroup" => Self::LogGroup,
            "AWS::ApiGateway::RestApi" => Self::RestApi,
            "AWS::ApiGatewayV2::Api" => Self::HttpApi,
            "AWS::ApiGateway::Stage" | "AWS::ApiGatewayV2::Stage" => Self::ApiStage,
            "AWS::ECS::Service" => Self::EcsService,
            "AWS::ECS::TaskDefinition" => Self::EcsTaskDefinition,
            "AWS::StepFunctions::StateMachine" => Self::StateMachine,
            "AWS::DynamoDB::Table" => Self::DynamoDbTable,
            "AWS::SQS::Queue" => Self::SqsQueue,
            "AWS::SNS::Topic" => Self::SnsTopic,
            "AWS::S3::Bucket" => Self::S3Bucket,
            "AWS::Kinesis::Stream" => Self::KinesisStream,
            "AWS::Events::Rule" => Self::EventBridgeRule,
            _ => Self::Unknown,
        }
    }

    /// Column for the construct map layout (ingress / compute / data).
    pub fn map_column(&self) -> Option<&'static str> {
        match self {
            Self::RestApi | Self::HttpApi | Self::ApiStage | Self::EventBridgeRule => {
                Some("in")
            }
            Self::LambdaFunction
            | Self::EcsService
            | Self::EcsTaskDefinition
            | Self::StateMachine => Some("compute"),
            Self::DynamoDbTable
            | Self::SqsQueue
            | Self::SnsTopic
            | Self::S3Bucket
            | Self::KinesisStream => Some("data"),
            Self::LogGroup | Self::Unknown => None,
        }
    }
}

/// A dependency edge between two constructs (`from` references `to`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEdge {
    /// Construct path of the resource that holds the Ref / GetAtt.
    pub from: String,
    /// Construct path of the referenced resource.
    pub to: String,
    /// How the link was discovered (`Ref` or `GetAtt`).
    pub via: String,
}

/// How a log-group mapping was derived (FR-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Read from an explicit `LogGroupName` / log config / access-log destination.
    Resolved,
    /// Derived conventionally (e.g. `/aws/lambda/<function-name>`).
    Inferred,
}

/// A single node in the resource graph: an AWS resource discovered from
/// `tree.json` / `manifest.json` / synthesized templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceNode {
    /// Construct path without trailing `/Resource`, e.g. `SampleStack/CreateOrder`.
    pub construct_path: String,
    /// CloudFormation resource type, e.g. `AWS::Lambda::Function`.
    pub cfn_type: String,
    pub kind: ResourceKind,
    /// CloudFormation logical ID (from manifest, or matched in templates).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_id: Option<String>,
    /// L2 construct FQN (e.g. `aws-cdk-lib.aws_lambda.Function`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fqn: Option<String>,
    /// Resolved CloudWatch log group, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_group: Option<String>,
    /// How `log_group` was derived (set whenever `log_group` is set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Confidence>,
    /// Lambda function name (from `FunctionName` / `functionName`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,
    /// Lambda runtime (from `Runtime` / `runtime`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    /// Physical / logical name for data-plane resources (table, queue, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_name: Option<String>,
}

/// Owns the nodes and dependency edges of the resource graph.
#[derive(Debug, Default, Serialize)]
pub struct ResourceGraph {
    nodes: Vec<ResourceNode>,
    #[serde(default)]
    edges: Vec<ResourceEdge>,
}

impl ResourceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, node: ResourceNode) {
        self.nodes.push(node);
    }

    pub fn nodes(&self) -> &[ResourceNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[ResourceEdge] {
        &self.edges
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Nodes with a resolved log group — the entry points for debugging.
    pub fn log_groups(&self) -> impl Iterator<Item = &ResourceNode> {
        self.nodes.iter().filter(|n| n.log_group.is_some())
    }

    /// Find the node for an exact construct path.
    pub fn find(&self, construct_path: &str) -> Option<&ResourceNode> {
        self.nodes
            .iter()
            .find(|n| n.construct_path == construct_path)
    }

    /// Build the graph from a `cdk.out/` directory.
    ///
    /// Reads `tree.json`, `manifest.json`, and every `*.template.json`
    /// referenced by the manifest (plus any other `*.template.json` in the
    /// directory as a fallback).
    pub fn from_cdk_out(dir: &Path) -> Result<Self, GraphError> {
        if !dir.is_dir() {
            return Err(GraphError::MissingDir {
                path: dir.display().to_string(),
            });
        }
        let tree_path = dir.join("tree.json");
        let manifest_path = dir.join("manifest.json");

        let tree_raw = read_file(&tree_path)?;
        let tree_file: TreeFile = serde_json::from_str(&tree_raw).map_err(|e| {
            GraphError::InvalidJson {
                file: tree_path.display().to_string(),
                reason: e.to_string(),
            }
        })?;

        let manifest_raw = read_file(&manifest_path)?;
        let manifest: ManifestFile = serde_json::from_str(&manifest_raw).map_err(|e| {
            GraphError::InvalidJson {
                file: manifest_path.display().to_string(),
                reason: e.to_string(),
            }
        })?;

        let logical_ids = logical_id_map(&manifest);
        let templates = load_templates(dir, &manifest)?;

        // 1. Walk the construct tree.
        let mut raw_nodes = Vec::new();
        walk_tree(&tree_file.tree, None, None, &mut raw_nodes);

        // 2. Join each node with its template resource.
        let mut nodes: Vec<ResourceNode> = raw_nodes
            .into_iter()
            .map(|raw| {
                let logical_id = logical_ids.get(&raw.full_path).cloned().or_else(|| {
                    fallback_logical_id(&templates, &raw.cfn_type, raw.tree_props.as_ref())
                });
                let template_props = logical_id
                    .as_ref()
                    .and_then(|id| templates.get(id))
                    .and_then(|r| r.properties.clone());

                let (log_group, confidence) =
                    extract_log_group(&raw.cfn_type, template_props.as_ref(), raw.tree_props.as_ref());

                // Lambda extras for the debugger (invoke command, node cards).
                let (function_name, runtime) = if raw.cfn_type == "AWS::Lambda::Function" {
                    (
                        str_prop(template_props.as_ref(), &["FunctionName"])
                            .or_else(|| str_prop(raw.tree_props.as_ref(), &["functionName"])),
                        str_prop(template_props.as_ref(), &["Runtime"])
                            .or_else(|| str_prop(raw.tree_props.as_ref(), &["runtime"])),
                    )
                } else {
                    (None, None)
                };

                let resource_name = extract_resource_name(
                    &raw.cfn_type,
                    template_props.as_ref(),
                    raw.tree_props.as_ref(),
                );

                ResourceNode {
                    construct_path: raw.display_path,
                    cfn_type: raw.cfn_type.clone(),
                    kind: ResourceKind::from_cfn_type(&raw.cfn_type),
                    logical_id,
                    fqn: raw.effective_fqn,
                    log_group,
                    confidence,
                    function_name,
                    runtime,
                    resource_name,
                }
            })
            .collect();

        // 3. Promote Lambda `Inferred` → `Resolved` when an explicit
        //    LogGroup with the same name exists (FR-7 + FR-8 linkage).
        let resolved: HashSet<String> = nodes
            .iter()
            .filter(|n| n.confidence == Some(Confidence::Resolved))
            .filter_map(|n| n.log_group.clone())
            .collect();
        for node in &mut nodes {
            if node.kind == ResourceKind::LambdaFunction
                && node.confidence == Some(Confidence::Inferred)
                && let Some(ref group) = node.log_group
                && resolved.contains(group)
            {
                node.confidence = Some(Confidence::Resolved);
            }
        }

        // 4. Dependency edges from Ref / Fn::GetAtt in template properties.
        let edges = extract_edges(&nodes, &templates);

        nodes.sort_by(|a, b| a.construct_path.cmp(&b.construct_path));
        Ok(Self { nodes, edges })
    }
}

fn read_file(path: &Path) -> Result<String, GraphError> {
    std::fs::read_to_string(path).map_err(|e| GraphError::Io {
        file: path.display().to_string(),
        source: e,
    })
}

// --- tree.json ---

#[derive(Debug, Deserialize)]
struct TreeFile {
    tree: TreeNode,
    #[allow(dead_code)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TreeNode {
    id: String,
    path: String,
    #[serde(default)]
    children: HashMap<String, TreeNode>,
    #[serde(default)]
    attributes: HashMap<String, serde_json::Value>,
    #[serde(rename = "constructInfo", default)]
    construct_info: Option<ConstructInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConstructInfo {
    fqn: Option<String>,
    #[allow(dead_code)]
    version: Option<String>,
}

struct RawNode {
    /// Full tree path including trailing `/Resource`.
    full_path: String,
    /// Display path with trailing `/Resource` collapsed.
    display_path: String,
    cfn_type: String,
    tree_props: Option<serde_json::Value>,
    effective_fqn: Option<String>,
}

fn walk_tree(
    node: &TreeNode,
    parent_path: Option<&str>,
    parent_fqn: Option<&str>,
    out: &mut Vec<RawNode>,
) {
    let own_fqn = node
        .construct_info
        .as_ref()
        .and_then(|c| c.fqn.clone());
    if let Some(cfn_type) = node
        .attributes
        .get("aws:cdk:cloudformation:type")
        .and_then(|v| v.as_str())
    {
        let tree_props = node
            .attributes
            .get("aws:cdk:cloudformation:props")
            .cloned();
        let display_path = if node.id == "Resource" {
            parent_path.unwrap_or(&node.path).to_string()
        } else {
            node.path.clone()
        };
        // Prefer the L2 parent FQN (`aws_lambda.Function`) over the
        // low-level `CfnXxx` FQN for `.../Resource` children.
        let effective_fqn = if node.id == "Resource" {
            parent_fqn.map(str::to_string).or(own_fqn.clone())
        } else {
            own_fqn.clone()
        };
        out.push(RawNode {
            full_path: node.path.clone(),
            display_path,
            cfn_type: cfn_type.to_string(),
            tree_props,
            effective_fqn,
        });
    }
    let child_parent_path = Some(node.path.as_str());
    let child_parent_fqn = own_fqn
        .as_deref()
        .or(parent_fqn)
        .map(str::to_string);
    // Sort children for deterministic traversal.
    let mut names: Vec<&String> = node.children.keys().collect();
    names.sort();
    for name in names {
        if let Some(child) = node.children.get(name) {
            walk_tree(
                child,
                child_parent_path,
                child_parent_fqn.as_deref().or(parent_fqn),
                out,
            );
        }
    }
}

// --- manifest.json ---

#[derive(Debug, Deserialize)]
struct ManifestFile {
    #[serde(default)]
    artifacts: HashMap<String, ManifestArtifact>,
}

#[derive(Debug, Deserialize)]
struct ManifestArtifact {
    #[serde(rename = "type")]
    _artifact_type: Option<String>,
    #[serde(default)]
    properties: Option<ManifestProps>,
    #[serde(default)]
    metadata: Option<HashMap<String, Vec<MetadataEntry>>>,
}

#[derive(Debug, Deserialize)]
struct ManifestProps {
    #[serde(rename = "templateFile")]
    template_file: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MetadataEntry {
    #[serde(rename = "type")]
    entry_type: String,
    data: serde_json::Value,
}

/// `SampleStack/CreateOrder/Resource` → `CreateOrder7AA1FBAB`.
fn logical_id_map(manifest: &ManifestFile) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for artifact in manifest.artifacts.values() {
        if let Some(metadata) = &artifact.metadata {
            for (path, entries) in metadata {
                for entry in entries {
                    if entry.entry_type == "aws:cdk:logicalId"
                        && let Some(id) = entry.data.as_str()
                    {
                        map.insert(path.trim_start_matches('/').to_string(), id.to_string());
                    }
                }
            }
        }
    }
    map
}

// --- templates ---

#[derive(Debug, Clone, Deserialize)]
struct CfnResource {
    #[serde(rename = "Type")]
    resource_type: String,
    #[serde(rename = "Properties")]
    properties: Option<serde_json::Value>,
}

fn load_templates(
    dir: &Path,
    manifest: &ManifestFile,
) -> Result<HashMap<String, CfnResource>, GraphError> {
    let mut files: Vec<PathBuf> = Vec::new();
    for artifact in manifest.artifacts.values() {
        if let Some(props) = &artifact.properties
            && let Some(name) = &props.template_file
        {
            let p = dir.join(name);
            if !files.contains(&p) {
                files.push(p);
            }
        }
    }
    // Fallback: any other `*.template.json` in the directory.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json")
                && let Some(name) = p.file_name().and_then(|s| s.to_str())
                && name.ends_with(".template.json")
                && !files.contains(&p)
            {
                files.push(p);
            }
        }
    }

    let mut out = HashMap::new();
    for file in files {
        if !file.is_file() {
            continue;
        }
        let raw = read_file(&file)?;
        let parsed: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| GraphError::InvalidJson {
                file: file.display().to_string(),
                reason: e.to_string(),
            })?;
        let resources = parsed
            .get("Resources")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        for (logical_id, value) in resources {
            if let Ok(res) = serde_json::from_value::<CfnResource>(value) {
                out.insert(logical_id, res);
            }
        }
    }
    Ok(out)
}

/// Fallback when manifest has no `aws:cdk:logicalId` entry (e.g. hand-written
/// fixtures): match by CFN type + identifying prop.
fn fallback_logical_id(
    templates: &HashMap<String, CfnResource>,
    cfn_type: &str,
    tree_props: Option<&serde_json::Value>,
) -> Option<String> {
    let want_fn = tree_props
        .and_then(|p| p.get("functionName"))
        .and_then(|v| v.as_str());
    let want_lg = tree_props
        .and_then(|p| p.get("logGroupName"))
        .and_then(|v| v.as_str());
    let want_table = tree_props
        .and_then(|p| p.get("tableName"))
        .and_then(|v| v.as_str());
    let want_queue = tree_props
        .and_then(|p| p.get("queueName"))
        .and_then(|v| v.as_str());
    let mut candidates: Vec<&String> = templates
        .iter()
        .filter(|(_, r)| r.resource_type == cfn_type)
        .filter(|(_, r)| {
            if let Some(want) = want_fn {
                return r
                    .properties
                    .as_ref()
                    .and_then(|p| p.get("FunctionName"))
                    .and_then(|v| v.as_str())
                    == Some(want);
            }
            if let Some(want) = want_lg {
                return r
                    .properties
                    .as_ref()
                    .and_then(|p| p.get("LogGroupName"))
                    .and_then(|v| v.as_str())
                    == Some(want);
            }
            if let Some(want) = want_table {
                return r
                    .properties
                    .as_ref()
                    .and_then(|p| p.get("TableName"))
                    .and_then(|v| v.as_str())
                    == Some(want);
            }
            if let Some(want) = want_queue {
                return r
                    .properties
                    .as_ref()
                    .and_then(|p| p.get("QueueName"))
                    .and_then(|v| v.as_str())
                    == Some(want);
            }
            false
        })
        .map(|(id, _)| id)
        .collect();
    candidates.sort();
    candidates.into_iter().next().cloned()
}

/// Physical name for data-plane resources (used by span → construct matching).
fn extract_resource_name(
    cfn_type: &str,
    template_props: Option<&serde_json::Value>,
    tree_props: Option<&serde_json::Value>,
) -> Option<String> {
    match cfn_type {
        "AWS::DynamoDB::Table" => str_prop(template_props, &["TableName"])
            .or_else(|| str_prop(tree_props, &["tableName"])),
        "AWS::SQS::Queue" => str_prop(template_props, &["QueueName"])
            .or_else(|| str_prop(tree_props, &["queueName"])),
        "AWS::SNS::Topic" => str_prop(template_props, &["TopicName"])
            .or_else(|| str_prop(tree_props, &["topicName"])),
        "AWS::S3::Bucket" => str_prop(template_props, &["BucketName"])
            .or_else(|| str_prop(tree_props, &["bucketName"])),
        "AWS::Kinesis::Stream" => str_prop(template_props, &["Name"])
            .or_else(|| str_prop(tree_props, &["streamName"])),
        "AWS::Events::Rule" => str_prop(template_props, &["Name"])
            .or_else(|| str_prop(tree_props, &["ruleName"])),
        _ => None,
    }
}

/// Resources that dominate Ref counts and say nothing about the request path.
fn is_noise_cfn_type(cfn_type: &str) -> bool {
    matches!(
        cfn_type,
        "AWS::IAM::Role"
            | "AWS::IAM::Policy"
            | "AWS::IAM::ManagedPolicy"
            | "AWS::IAM::InstanceProfile"
            | "AWS::Logs::LogGroup"
            | "AWS::CDK::Metadata"
            | "AWS::Lambda::Permission"
            | "AWS::ApiGateway::Account"
            | "AWS::ApiGateway::Deployment"
    ) || cfn_type.starts_with("Custom::")
}

fn is_unresolved_token(s: &str) -> bool {
    s.contains("${Token[") || s.contains("DUMMY") || s.starts_with("${")
}

/// Walk template properties collecting `Ref` / `Fn::GetAtt` logical IDs.
fn collect_intrinsic_refs(value: &serde_json::Value, out: &mut Vec<(String, &'static str)>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(r) = map.get("Ref").and_then(|v| v.as_str()) {
                if !is_unresolved_token(r) && !r.starts_with("AWS::") {
                    out.push((r.to_string(), "Ref"));
                }
                return;
            }
            if let Some(get_att) = map.get("Fn::GetAtt") {
                let target = match get_att {
                    serde_json::Value::Array(arr) => arr.first().and_then(|v| v.as_str()),
                    serde_json::Value::String(s) => s.split('.').next(),
                    _ => None,
                };
                if let Some(id) = target
                    && !is_unresolved_token(id)
                {
                    out.push((id.to_string(), "GetAtt"));
                }
                return;
            }
            for v in map.values() {
                collect_intrinsic_refs(v, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                collect_intrinsic_refs(v, out);
            }
        }
        _ => {}
    }
}

fn extract_edges(
    nodes: &[ResourceNode],
    templates: &HashMap<String, CfnResource>,
) -> Vec<ResourceEdge> {
    let by_logical: HashMap<&str, &ResourceNode> = nodes
        .iter()
        .filter_map(|n| n.logical_id.as_deref().map(|id| (id, n)))
        .collect();

    let mut edges = Vec::new();
    let mut seen = HashSet::new();

    for node in nodes {
        let Some(logical_id) = node.logical_id.as_deref() else {
            continue;
        };
        if is_noise_cfn_type(&node.cfn_type) {
            continue;
        }
        let Some(res) = templates.get(logical_id) else {
            continue;
        };
        let Some(props) = res.properties.as_ref() else {
            continue;
        };

        let mut refs = Vec::new();
        collect_intrinsic_refs(props, &mut refs);

        for (target_id, via) in refs {
            let Some(target) = by_logical.get(target_id.as_str()) else {
                continue;
            };
            if is_noise_cfn_type(&target.cfn_type) {
                continue;
            }
            if target.construct_path == node.construct_path {
                continue;
            }
            let key = (
                node.construct_path.clone(),
                target.construct_path.clone(),
                via,
            );
            if !seen.insert(key.clone()) {
                continue;
            }
            edges.push(ResourceEdge {
                from: key.0,
                to: key.1,
                via: via.to_string(),
            });
        }
    }

    edges.sort_by(|a, b| {
        (&a.from, &a.to, &a.via).cmp(&(&b.from, &b.to, &b.via))
    });
    edges
}

// --- log-group extraction (FR-7 / FR-8) ---

fn extract_log_group(
    cfn_type: &str,
    template_props: Option<&serde_json::Value>,
    tree_props: Option<&serde_json::Value>,
) -> (Option<String>, Option<Confidence>) {
    match cfn_type {
        "AWS::Logs::LogGroup" => {
            if let Some(name) = str_prop(template_props, &["LogGroupName"]) {
                return (Some(name), Some(Confidence::Resolved));
            }
            if let Some(name) = str_prop(tree_props, &["logGroupName"]) {
                return (Some(name), Some(Confidence::Resolved));
            }
            (None, None)
        }
        "AWS::Lambda::Function" => {
            if let Some(name) = str_prop(template_props, &["FunctionName"])
                .or_else(|| str_prop(tree_props, &["functionName"]))
            {
                return (
                    Some(format!("/aws/lambda/{name}")),
                    Some(Confidence::Inferred),
                );
            }
            (None, None)
        }
        "AWS::ApiGateway::Stage" | "AWS::ApiGatewayV2::Stage" => {
            // Template: AccessLogSetting(s).DestinationArn; tree: accessLogSetting(s).destinationArn
            for key_path in [
                vec!["AccessLogSetting", "DestinationArn"],
                vec!["AccessLogSettings", "DestinationArn"],
                vec!["accessLogSetting", "destinationArn"],
                vec!["accessLogSettings", "destinationArn"],
            ] {
                if let Some(arn) = str_prop(template_props, &key_path)
                    .or_else(|| str_prop(tree_props, &key_path))
                    && let Some(group) = log_group_from_arn_or_name(&arn)
                {
                    return (Some(group), Some(Confidence::Resolved));
                }
            }
            (None, None)
        }
        "AWS::ECS::TaskDefinition" => {
            for props in [template_props, tree_props].into_iter().flatten() {
                if let Some(group) = first_awslogs_group(props) {
                    return (Some(group), Some(Confidence::Resolved));
                }
            }
            (None, None)
        }
        "AWS::StepFunctions::StateMachine" => {
            for props in [template_props, tree_props].into_iter().flatten() {
                if let Some(group) = first_sfn_log_group(props) {
                    return (Some(group), Some(Confidence::Resolved));
                }
            }
            (None, None)
        }
        _ => (None, None),
    }
}

fn str_prop(props: Option<&serde_json::Value>, path: &[&str]) -> Option<String> {
    let mut cur = props?;
    for key in path {
        // Match keys case-insensitively: templates use PascalCase,
        // tree props use camelCase.
        if let Some(obj) = cur.as_object() {
            let found = obj.iter().find(|(k, _)| k.eq_ignore_ascii_case(key));
            cur = found.map(|(_, v)| v)?;
        } else {
            return None;
        }
    }
    cur.as_str().map(str::to_string)
}

/// `arn:aws:logs:<region>:<acct>:log-group:<name>:*` → `<name>`.
/// A bare `/aws/...` value is returned as-is.
fn log_group_from_arn_or_name(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    if let Some(idx) = v.find(":log-group:") {
        let rest = &v[idx + ":log-group:".len()..];
        let name = rest.strip_suffix(":*").unwrap_or(rest);
        if name.is_empty() {
            return None;
        }
        return Some(name.to_string());
    }
    if v.starts_with('/') {
        return Some(v.to_string());
    }
    None
}

fn first_awslogs_group(props: &serde_json::Value) -> Option<String> {
    let containers = find_key_ci(props, "containerDefinitions")?;
    let arr = containers.as_array()?;
    for container in arr {
        for path in [
            vec!["logConfiguration", "options", "awslogs-group"],
            vec!["LogConfiguration", "Options", "awslogs-group"],
        ] {
            // Options keys are literal (`awslogs-group`); walk case-insensitively
            // except for the final key which is already lowercase.
            let mut cur: Option<&serde_json::Value> = Some(container);
            for key in &path {
                cur = cur.and_then(|v| find_key_ci(v, key));
            }
            if let Some(group) = cur.and_then(|v| v.as_str()) {
                return Some(group.to_string());
            }
        }
    }
    None
}

fn first_sfn_log_group(props: &serde_json::Value) -> Option<String> {
    let logging = find_key_ci(props, "loggingConfiguration")?;
    let dests = find_key_ci(logging, "destinations")?.as_array()?;
    for dest in dests {
        for path in [
            vec!["cloudWatchLogsLogGroup", "logGroupArn"],
            vec!["CloudWatchLogsLogGroup", "LogGroupArn"],
        ] {
            let mut cur: Option<&serde_json::Value> = Some(dest);
            for key in &path {
                cur = cur.and_then(|v| find_key_ci(v, key));
            }
            if let Some(arn) = cur.and_then(|v| v.as_str())
                && let Some(group) = log_group_from_arn_or_name(arn)
            {
                return Some(group);
            }
        }
    }
    None
}

fn find_key_ci<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    value.as_object()?.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case(key) {
            Some(v)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample-cdk-out")
    }

    #[test]
    fn graph_collects_nodes_and_log_groups() {
        let mut graph = ResourceGraph::new();
        assert!(graph.is_empty());

        graph.push(ResourceNode {
            construct_path: "SampleStack/CreateOrder".to_string(),
            cfn_type: "AWS::Lambda::Function".to_string(),
            kind: ResourceKind::from_cfn_type("AWS::Lambda::Function"),
            logical_id: None,
            fqn: None,
            log_group: Some("/aws/lambda/sample-create-order".to_string()),
            confidence: Some(Confidence::Inferred),
            function_name: Some("sample-create-order".to_string()),
            runtime: Some("nodejs22.x".to_string()),
            resource_name: None,
        });
        graph.push(ResourceNode {
            construct_path: "SampleStack/CreateOrder/Role".to_string(),
            cfn_type: "AWS::IAM::Role".to_string(),
            kind: ResourceKind::from_cfn_type("AWS::IAM::Role"),
            logical_id: None,
            fqn: None,
            log_group: None,
            confidence: None,
            function_name: None,
            runtime: None,
            resource_name: None,
        });

        assert_eq!(graph.len(), 2);
        let groups: Vec<_> = graph.log_groups().collect();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].construct_path, "SampleStack/CreateOrder");
    }

    #[test]
    fn parses_sample_cdk_out() {
        let graph = ResourceGraph::from_cdk_out(&fixture_dir()).unwrap();
        assert_eq!(graph.len(), 8);

        let by_path = |p: &str| {
            graph
                .find(p)
                .unwrap_or_else(|| panic!("missing node {p}"))
                .clone()
        };

        // Lambda with an explicit LogGroup → promoted to Resolved.
        let lambda = by_path("SampleStack/CreateOrder");
        assert_eq!(lambda.cfn_type, "AWS::Lambda::Function");
        assert_eq!(lambda.kind, ResourceKind::LambdaFunction);
        assert_eq!(lambda.logical_id.as_deref(), Some("CreateOrder7AA1FBAB"));
        assert_eq!(
            lambda.fqn.as_deref(),
            Some("aws-cdk-lib.aws_lambda.Function")
        );
        assert_eq!(
            lambda.log_group.as_deref(),
            Some("/aws/lambda/sample-create-order")
        );
        assert_eq!(lambda.confidence, Some(Confidence::Resolved));
        assert_eq!(lambda.function_name.as_deref(), Some("sample-create-order"));
        assert_eq!(lambda.runtime.as_deref(), Some("nodejs22.x"));

        let log_group = by_path("SampleStack/CreateOrderLogGroup");
        assert_eq!(log_group.kind, ResourceKind::LogGroup);
        assert_eq!(
            log_group.log_group.as_deref(),
            Some("/aws/lambda/sample-create-order")
        );
        assert_eq!(log_group.confidence, Some(Confidence::Resolved));

        // Lambda without an explicit group → Inferred.
        let inferred = by_path("SampleStack/InferredWorker");
        assert_eq!(
            inferred.log_group.as_deref(),
            Some("/aws/lambda/sample-inferred-worker")
        );
        assert_eq!(inferred.confidence, Some(Confidence::Inferred));

        // API Gateway access logs.
        let stage = by_path("SampleStack/OrdersApiStage");
        assert_eq!(stage.kind, ResourceKind::ApiStage);
        assert_eq!(
            stage.log_group.as_deref(),
            Some("/aws/apigateway/orders-access-logs")
        );
        assert_eq!(stage.confidence, Some(Confidence::Resolved));

        // ECS awslogs-group.
        let task_def = by_path("SampleStack/WorkerTaskDef");
        assert_eq!(task_def.kind, ResourceKind::EcsTaskDefinition);
        assert_eq!(task_def.log_group.as_deref(), Some("/ecs/worker"));
        assert_eq!(task_def.confidence, Some(Confidence::Resolved));

        // Step Functions logging config.
        let sfn = by_path("SampleStack/ProcessOrders");
        assert_eq!(sfn.kind, ResourceKind::StateMachine);
        assert_eq!(
            sfn.log_group.as_deref(),
            Some("/aws/states/process-orders")
        );
        assert_eq!(sfn.confidence, Some(Confidence::Resolved));

        // Data-plane nodes (no log groups).
        let table = by_path("SampleStack/OrdersTable");
        assert_eq!(table.kind, ResourceKind::DynamoDbTable);
        assert_eq!(table.resource_name.as_deref(), Some("sample-orders"));
        assert!(table.log_group.is_none());

        let queue = by_path("SampleStack/OrdersQueue");
        assert_eq!(queue.kind, ResourceKind::SqsQueue);
        assert_eq!(queue.resource_name.as_deref(), Some("sample-orders-queue"));

        // CreateOrder → table / queue via Env Ref / GetAtt.
        let edge_tos: HashSet<&str> = graph
            .edges()
            .iter()
            .filter(|e| e.from == "SampleStack/CreateOrder")
            .map(|e| e.to.as_str())
            .collect();
        assert!(edge_tos.contains("SampleStack/OrdersTable"));
        assert!(edge_tos.contains("SampleStack/OrdersQueue"));
        assert!(graph.edges().iter().all(|e| e.via == "Ref" || e.via == "GetAtt"));

        // Every mapped group is reachable via log_groups().
        assert_eq!(graph.log_groups().count(), 6);
    }

    #[test]
    fn parses_arn_variants() {
        assert_eq!(
            log_group_from_arn_or_name(
                "arn:aws:logs:us-east-1:123456789012:log-group:/aws/lambda/foo:*"
            ),
            Some("/aws/lambda/foo".to_string())
        );
        assert_eq!(
            log_group_from_arn_or_name("/aws/lambda/bare"),
            Some("/aws/lambda/bare".to_string())
        );
        assert_eq!(log_group_from_arn_or_name(""), None);
        assert_eq!(log_group_from_arn_or_name("not-a-group"), None);
    }

    #[test]
    fn errors_on_missing_dir() {
        let err =
            ResourceGraph::from_cdk_out(Path::new("/does/not/exist-cw-trace")).unwrap_err();
        assert!(matches!(err, GraphError::MissingDir { .. }));
    }
}
