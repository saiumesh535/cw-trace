//! Normalized resource graph built from CDK synthesis output (`cdk.out/`).
//!
//! The graph maps CDK construct paths (e.g. `OrdersStack/OrdersApi/CreateOrder`)
//! to the AWS resources they produce and the CloudWatch log groups those
//! resources write to — so the debugger never has to guess a log group name.
//!
//! See `docs/PRD.md` for the product context.

/// An AWS resource kind relevant to log/trace debugging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    LambdaFunction,
    LogGroup,
    RestApi,
    HttpApi,
    EcsService,
    StateMachine,
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
            "AWS::ECS::Service" => Self::EcsService,
            "AWS::StepFunctions::StateMachine" => Self::StateMachine,
            _ => Self::Unknown,
        }
    }
}

/// A single node in the resource graph: an AWS resource discovered from
/// `tree.json` / `manifest.json` / synthesized templates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceNode {
    /// Full construct path, e.g. `SampleStack/CreateOrder`.
    pub construct_path: String,
    /// CloudFormation resource type, e.g. `AWS::Lambda::Function`.
    pub cfn_type: String,
    pub kind: ResourceKind,
    /// Resolved CloudWatch log group, when known.
    pub log_group: Option<String>,
}

/// Owns the nodes of the resource graph. Edges (e.g. Lambda → LogGroup)
/// are derived on demand in M1.
#[derive(Debug, Default)]
pub struct ResourceGraph {
    nodes: Vec<ResourceNode>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_collects_nodes_and_log_groups() {
        let mut graph = ResourceGraph::new();
        assert!(graph.is_empty());

        graph.push(ResourceNode {
            construct_path: "SampleStack/CreateOrder".to_string(),
            cfn_type: "AWS::Lambda::Function".to_string(),
            kind: ResourceKind::from_cfn_type("AWS::Lambda::Function"),
            log_group: Some("/aws/lambda/sample-create-order".to_string()),
        });
        graph.push(ResourceNode {
            construct_path: "SampleStack/CreateOrder/Role".to_string(),
            cfn_type: "AWS::IAM::Role".to_string(),
            kind: ResourceKind::from_cfn_type("AWS::IAM::Role"),
            log_group: None,
        });

        assert_eq!(graph.len(), 2);
        let groups: Vec<_> = graph.log_groups().collect();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].construct_path, "SampleStack/CreateOrder");
    }
}
