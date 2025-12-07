use super::api::{RunFlowRequest, RunFlowResult, RunnerApi};
use super::error::{GResult, RunnerError};
use super::host::HostBundle;
use super::policy::Policy;
use super::registry::AdapterRegistry;
use super::state_machine::{FlowDefinition, StateMachine};
use async_trait::async_trait;
use greentic_types::TenantCtx;
use std::sync::Arc;

#[derive(Default)]
pub struct RunnerBuilder {
    host: Option<HostBundle>,
    adapters: Option<AdapterRegistry>,
    policy: Option<Policy>,
    flows: Vec<FlowDefinition>,
}

impl RunnerBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_host(mut self, host: HostBundle) -> Self {
        self.host = Some(host);
        self
    }

    pub fn with_adapters(mut self, adapters: AdapterRegistry) -> Self {
        self.adapters = Some(adapters);
        self
    }

    pub fn with_policy(mut self, policy: Policy) -> Self {
        self.policy = Some(policy);
        self
    }

    pub fn with_flow(mut self, definition: FlowDefinition) -> Self {
        self.flows.push(definition);
        self
    }

    pub fn build(self) -> GResult<Runner> {
        let host = self.host.ok_or_else(|| RunnerError::Policy {
            reason: "host bundle missing".into(),
        })?;
        let adapters = self.adapters.unwrap_or_default();
        let policy = self.policy.unwrap_or_default();
        let state_machine = StateMachine::new(Arc::new(host), adapters, policy);
        for flow in self.flows {
            state_machine.register_flow(flow);
        }
        Ok(Runner { sm: state_machine })
    }
}

pub struct Runner {
    sm: StateMachine,
}

impl Runner {
    pub fn state_machine(&self) -> &StateMachine {
        &self.sm
    }

    pub fn state_machine_mut(&mut self) -> &mut StateMachine {
        &mut self.sm
    }

    pub fn into_state_machine(self) -> StateMachine {
        self.sm
    }
}

#[async_trait]
impl RunnerApi for Runner {
    async fn list_flows(&self, _tenant: &TenantCtx) -> GResult<Vec<super::api::FlowSummary>> {
        Ok(self.sm.list_flows())
    }

    async fn get_flow_schema(
        &self,
        _tenant: &TenantCtx,
        flow_id: &str,
    ) -> GResult<super::api::FlowSchema> {
        self.sm.get_flow_schema(flow_id)
    }

    async fn run_flow(&self, req: RunFlowRequest) -> GResult<RunFlowResult> {
        let outcome = self
            .sm
            .step(
                &req.tenant,
                &req.flow_id,
                req.session_hint.clone(),
                req.input.clone(),
            )
            .await?;
        Ok(RunFlowResult { outcome })
    }
}
