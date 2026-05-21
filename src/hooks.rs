// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Lifecycle Hook Framework for intercepting, observing, and modifying agent sessions.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::types::{
    AskQuestionInteractionSpec, Content, ToolCall, ToolResult, UserQuestionsResponse,
};

// =============================================================================
// Hook Context Management
// =============================================================================

#[derive(Clone, Default)]
pub struct HookContextStore {
    store: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    parent: Option<Arc<HookContextStore>>,
}

impl HookContextStore {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
            parent: None,
        }
    }

    pub fn with_parent(parent: Arc<HookContextStore>) -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
            parent: Some(parent),
        }
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        if let Some(val) = self.store.read().unwrap().get(key).cloned() {
            return Some(val);
        }
        if let Some(ref p) = self.parent {
            return p.get(key);
        }
        None
    }

    pub fn set(&self, key: &str, value: serde_json::Value) {
        self.store.write().unwrap().insert(key.to_string(), value);
    }
}

/// Context scoped to an entire agent session.
#[derive(Clone, Default)]
pub struct SessionContext {
    pub(crate) store: Arc<HookContextStore>,
}

impl SessionContext {
    pub fn new() -> Self {
        Self {
            store: Arc::new(HookContextStore::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.store.get(key)
    }

    pub fn set(&self, key: &str, value: serde_json::Value) {
        self.store.set(key, value);
    }
}

/// Context scoped to a single conversation turn (prompt/response cycle).
#[derive(Clone)]
pub struct TurnContext {
    pub(crate) store: Arc<HookContextStore>,
}

impl TurnContext {
    pub fn new(session: &SessionContext) -> Self {
        Self {
            store: Arc::new(HookContextStore::with_parent(session.store.clone())),
        }
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.store.get(key)
    }

    pub fn set(&self, key: &str, value: serde_json::Value) {
        self.store.set(key, value);
    }
}

/// Context scoped to a single operation (e.g., a tool call).
#[derive(Clone)]
pub struct OperationContext {
    pub(crate) store: Arc<HookContextStore>,
}

impl OperationContext {
    pub fn new(turn: &TurnContext) -> Self {
        Self {
            store: Arc::new(HookContextStore::with_parent(turn.store.clone())),
        }
    }

    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.store.get(key)
    }

    pub fn set(&self, key: &str, value: serde_json::Value) {
        self.store.set(key, value);
    }
}

// =============================================================================
// Hook Result
// =============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HookResult {
    pub allow: bool,
    pub message: String,
}

impl HookResult {
    pub fn new(allow: bool, message: &str) -> Self {
        Self {
            allow,
            message: message.to_string(),
        }
    }

    pub fn allow() -> Self {
        Self {
            allow: true,
            message: String::new(),
        }
    }

    pub fn deny(message: &str) -> Self {
        Self {
            allow: false,
            message: message.to_string(),
        }
    }
}

impl Default for HookResult {
    fn default() -> Self {
        Self::allow()
    }
}

// =============================================================================
// Hook Traits
// =============================================================================

pub type HookFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

// =============================================================================
// Hook Traits
// =============================================================================

/// Invoked when the session starts.
pub trait OnSessionStart: Send + Sync {
    fn run<'a>(&'a self, context: &'a SessionContext) -> HookFuture<'a, Result<(), String>>;
}

/// Invoked when the session ends.
pub trait OnSessionEnd: Send + Sync {
    fn run<'a>(&'a self, context: &'a SessionContext) -> HookFuture<'a, Result<(), String>>;
}

/// Invoked before a turn starts.
pub trait PreTurn: Send + Sync {
    fn run<'a>(
        &'a self,
        context: &'a TurnContext,
        prompt: &'a Content,
    ) -> HookFuture<'a, Result<HookResult, String>>;
}

/// Invoked after a turn ends.
pub trait PostTurn: Send + Sync {
    fn run<'a>(
        &'a self,
        context: &'a TurnContext,
        response: &'a str,
    ) -> HookFuture<'a, Result<(), String>>;
}

/// Invoked before a tool call to decide if it should proceed.
pub trait PreToolCallDecide: Send + Sync {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        tool_call: &'a ToolCall,
    ) -> HookFuture<'a, Result<HookResult, String>>;
}

/// Invoked after a tool call completes.
pub trait PostToolCall: Send + Sync {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        result: &'a ToolResult,
    ) -> HookFuture<'a, Result<(), String>>;
}

/// Invoked when a tool fails, allowing recovery or modification.
pub trait OnToolError: Send + Sync {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        error: &'a (dyn std::error::Error + Send + Sync),
    ) -> HookFuture<'a, Result<Option<serde_json::Value>, String>>;
}

/// Hook invoked when the agent needs user interaction.
pub trait OnInteraction: Send + Sync {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        spec: &'a AskQuestionInteractionSpec,
    ) -> HookFuture<'a, Result<Option<UserQuestionsResponse>, String>>;
}

/// Invoked when a context compaction event occurs.
pub trait OnCompaction: Send + Sync {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        data: &'a serde_json::Value,
    ) -> HookFuture<'a, Result<(), String>>;
}

// =============================================================================
// Closure blanket implementations
// =============================================================================

impl<F, Fut> OnSessionStart for F
where
    F: Fn(SessionContext) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    fn run<'a>(&'a self, context: &'a SessionContext) -> HookFuture<'a, Result<(), String>> {
        Box::pin((self)(context.clone()))
    }
}

impl<F, Fut> OnSessionEnd for F
where
    F: Fn(SessionContext) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    fn run<'a>(&'a self, context: &'a SessionContext) -> HookFuture<'a, Result<(), String>> {
        Box::pin((self)(context.clone()))
    }
}

impl<F, Fut> PreTurn for F
where
    F: Fn(TurnContext, Content) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<HookResult, String>> + Send + 'static,
{
    fn run<'a>(
        &'a self,
        context: &'a TurnContext,
        prompt: &'a Content,
    ) -> HookFuture<'a, Result<HookResult, String>> {
        Box::pin((self)(context.clone(), prompt.clone()))
    }
}

impl<F, Fut> PostTurn for F
where
    F: Fn(TurnContext, String) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    fn run<'a>(
        &'a self,
        context: &'a TurnContext,
        response: &'a str,
    ) -> HookFuture<'a, Result<(), String>> {
        Box::pin((self)(context.clone(), response.to_string()))
    }
}

impl<F, Fut> PreToolCallDecide for F
where
    F: Fn(OperationContext, ToolCall) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<HookResult, String>> + Send + 'static,
{
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        tool_call: &'a ToolCall,
    ) -> HookFuture<'a, Result<HookResult, String>> {
        Box::pin((self)(context.clone(), tool_call.clone()))
    }
}

impl<F, Fut> PostToolCall for F
where
    F: Fn(OperationContext, ToolResult) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        result: &'a ToolResult,
    ) -> HookFuture<'a, Result<(), String>> {
        Box::pin((self)(context.clone(), result.clone()))
    }
}

impl<F, Fut> OnToolError for F
where
    F: Fn(OperationContext, String) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<Option<serde_json::Value>, String>> + Send + 'static,
{
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        error: &'a (dyn std::error::Error + Send + Sync),
    ) -> HookFuture<'a, Result<Option<serde_json::Value>, String>> {
        Box::pin((self)(context.clone(), error.to_string()))
    }
}

impl<F, Fut> OnInteraction for F
where
    F: Fn(OperationContext, AskQuestionInteractionSpec) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<Option<UserQuestionsResponse>, String>>
        + Send
        + 'static,
{
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        spec: &'a AskQuestionInteractionSpec,
    ) -> HookFuture<'a, Result<Option<UserQuestionsResponse>, String>> {
        Box::pin((self)(context.clone(), spec.clone()))
    }
}

impl<F, Fut> OnCompaction for F
where
    F: Fn(OperationContext, serde_json::Value) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        data: &'a serde_json::Value,
    ) -> HookFuture<'a, Result<(), String>> {
        Box::pin((self)(context.clone(), data.clone()))
    }
}

// =============================================================================
// Arc blanket implementations
// =============================================================================

impl<T: OnSessionStart + ?Sized> OnSessionStart for Arc<T> {
    fn run<'a>(&'a self, context: &'a SessionContext) -> HookFuture<'a, Result<(), String>> {
        (**self).run(context)
    }
}

impl<T: OnSessionEnd + ?Sized> OnSessionEnd for Arc<T> {
    fn run<'a>(&'a self, context: &'a SessionContext) -> HookFuture<'a, Result<(), String>> {
        (**self).run(context)
    }
}

impl<T: PreTurn + ?Sized> PreTurn for Arc<T> {
    fn run<'a>(
        &'a self,
        context: &'a TurnContext,
        prompt: &'a Content,
    ) -> HookFuture<'a, Result<HookResult, String>> {
        (**self).run(context, prompt)
    }
}

impl<T: PostTurn + ?Sized> PostTurn for Arc<T> {
    fn run<'a>(
        &'a self,
        context: &'a TurnContext,
        response: &'a str,
    ) -> HookFuture<'a, Result<(), String>> {
        (**self).run(context, response)
    }
}

impl<T: PreToolCallDecide + ?Sized> PreToolCallDecide for Arc<T> {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        tool_call: &'a ToolCall,
    ) -> HookFuture<'a, Result<HookResult, String>> {
        (**self).run(context, tool_call)
    }
}

impl<T: PostToolCall + ?Sized> PostToolCall for Arc<T> {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        result: &'a ToolResult,
    ) -> HookFuture<'a, Result<(), String>> {
        (**self).run(context, result)
    }
}

impl<T: OnToolError + ?Sized> OnToolError for Arc<T> {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        error: &'a (dyn std::error::Error + Send + Sync),
    ) -> HookFuture<'a, Result<Option<serde_json::Value>, String>> {
        (**self).run(context, error)
    }
}

impl<T: OnInteraction + ?Sized> OnInteraction for Arc<T> {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        spec: &'a AskQuestionInteractionSpec,
    ) -> HookFuture<'a, Result<Option<UserQuestionsResponse>, String>> {
        (**self).run(context, spec)
    }
}

impl<T: OnCompaction + ?Sized> OnCompaction for Arc<T> {
    fn run<'a>(
        &'a self,
        context: &'a OperationContext,
        data: &'a serde_json::Value,
    ) -> HookFuture<'a, Result<(), String>> {
        (**self).run(context, data)
    }
}

// =============================================================================
// Hook Runner
// =============================================================================

#[derive(Clone, Default)]
pub struct HookRunner {
    pub session_context: SessionContext,
    pub on_session_start_hooks: Vec<Arc<dyn OnSessionStart>>,
    pub on_session_end_hooks: Vec<Arc<dyn OnSessionEnd>>,
    pub pre_turn_hooks: Vec<Arc<dyn PreTurn>>,
    pub post_turn_hooks: Vec<Arc<dyn PostTurn>>,
    pub pre_tool_call_decide_hooks: Vec<Arc<dyn PreToolCallDecide>>,
    pub post_tool_call_hooks: Vec<Arc<dyn PostToolCall>>,
    pub on_tool_error_hooks: Vec<Arc<dyn OnToolError>>,
    pub on_interaction_hooks: Vec<Arc<dyn OnInteraction>>,
    pub on_compaction_hooks: Vec<Arc<dyn OnCompaction>>,
}

impl HookRunner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_hooks(&self) -> bool {
        !(self.on_session_start_hooks.is_empty()
            && self.on_session_end_hooks.is_empty()
            && self.pre_turn_hooks.is_empty()
            && self.post_turn_hooks.is_empty()
            && self.pre_tool_call_decide_hooks.is_empty()
            && self.post_tool_call_hooks.is_empty()
            && self.on_tool_error_hooks.is_empty()
            && self.on_interaction_hooks.is_empty()
            && self.on_compaction_hooks.is_empty())
    }

    pub fn register_on_session_start<H: OnSessionStart + 'static>(&mut self, hook: H) {
        self.on_session_start_hooks.push(Arc::new(hook));
    }

    pub fn register_on_session_end<H: OnSessionEnd + 'static>(&mut self, hook: H) {
        self.on_session_end_hooks.push(Arc::new(hook));
    }

    pub fn register_pre_turn<H: PreTurn + 'static>(&mut self, hook: H) {
        self.pre_turn_hooks.push(Arc::new(hook));
    }

    pub fn register_post_turn<H: PostTurn + 'static>(&mut self, hook: H) {
        self.post_turn_hooks.push(Arc::new(hook));
    }

    pub fn register_pre_tool_call_decide<H: PreToolCallDecide + 'static>(&mut self, hook: H) {
        self.pre_tool_call_decide_hooks.push(Arc::new(hook));
    }

    pub fn register_post_tool_call<H: PostToolCall + 'static>(&mut self, hook: H) {
        self.post_tool_call_hooks.push(Arc::new(hook));
    }

    pub fn register_on_tool_error<H: OnToolError + 'static>(&mut self, hook: H) {
        self.on_tool_error_hooks.push(Arc::new(hook));
    }

    pub fn register_on_interaction<H: OnInteraction + 'static>(&mut self, hook: H) {
        self.on_interaction_hooks.push(Arc::new(hook));
    }

    pub fn register_on_compaction<H: OnCompaction + 'static>(&mut self, hook: H) {
        self.on_compaction_hooks.push(Arc::new(hook));
    }

    // --- Dispatchers ---

    pub async fn dispatch_session_start(&self) -> Result<(), String> {
        for hook in &self.on_session_start_hooks {
            hook.run(&self.session_context).await?;
        }
        Ok(())
    }

    pub async fn dispatch_session_end(&self) -> Result<(), String> {
        for hook in &self.on_session_end_hooks {
            hook.run(&self.session_context).await?;
        }
        Ok(())
    }

    pub async fn dispatch_pre_turn(
        &self,
        prompt: &Option<Content>,
    ) -> Result<(HookResult, TurnContext), String> {
        let turn_ctx = TurnContext::new(&self.session_context);
        let normalized_prompt = prompt.clone().unwrap_or_default();
        for hook in &self.pre_turn_hooks {
            let res = hook.run(&turn_ctx, &normalized_prompt).await?;
            if !res.allow {
                return Ok((res, turn_ctx));
            }
        }
        Ok((HookResult::allow(), turn_ctx))
    }

    pub async fn dispatch_post_turn(
        &self,
        turn_context: &TurnContext,
        response: &str,
    ) -> Result<(), String> {
        for hook in &self.post_turn_hooks {
            hook.run(turn_context, response).await?;
        }
        Ok(())
    }

    pub async fn dispatch_pre_tool_call(
        &self,
        turn_context: &TurnContext,
        tool_call: &ToolCall,
    ) -> Result<(HookResult, ToolCall, OperationContext), String> {
        let op_ctx = OperationContext::new(turn_context);
        for hook in &self.pre_tool_call_decide_hooks {
            let res = hook.run(&op_ctx, tool_call).await?;
            if !res.allow {
                return Ok((res, tool_call.clone(), op_ctx));
            }
        }
        Ok((HookResult::allow(), tool_call.clone(), op_ctx))
    }

    pub async fn dispatch_post_tool_call(
        &self,
        op_context: &OperationContext,
        result: &ToolResult,
    ) -> Result<(), String> {
        for hook in &self.post_tool_call_hooks {
            hook.run(op_context, result).await?;
        }
        Ok(())
    }

    pub async fn dispatch_on_tool_error(
        &self,
        op_context: &OperationContext,
        error: &(dyn std::error::Error + Send + Sync),
    ) -> Result<(HookResult, Option<serde_json::Value>), String> {
        for hook in &self.on_tool_error_hooks {
            match hook.run(op_context, error).await {
                Ok(Some(val)) => return Ok((HookResult::allow(), Some(val))),
                Ok(None) => {}
                Err(e) => {
                    log::error!("Critical failure in OnToolErrorHook: {}", e);
                    return Ok((
                        HookResult::deny(&format!("Error recovery failed: {}", e)),
                        None,
                    ));
                }
            }
        }
        Ok((HookResult::deny(""), None))
    }

    pub async fn dispatch_interaction(
        &self,
        turn_context: &TurnContext,
        spec: &AskQuestionInteractionSpec,
    ) -> Result<(HookResult, Option<UserQuestionsResponse>, OperationContext), String> {
        let op_ctx = OperationContext::new(turn_context);
        for hook in &self.on_interaction_hooks {
            if let Some(res) = hook.run(&op_ctx, spec).await? {
                return Ok((HookResult::allow(), Some(res), op_ctx));
            }
        }
        Ok((
            HookResult::deny("No interaction hook handled the request"),
            None,
            op_ctx,
        ))
    }

    pub async fn dispatch_compaction(
        &self,
        turn_context: &TurnContext,
        data: &serde_json::Value,
    ) -> Result<(), String> {
        let op_ctx = OperationContext::new(turn_context);
        for hook in &self.on_compaction_hooks {
            hook.run(&op_ctx, data).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Content, ToolCall};
    use std::sync::Mutex;

    #[test]
    fn test_context_hierarchy() {
        let session = SessionContext::new();
        session.set("session_key", serde_json::json!("session_val"));
        session.set("override_key", serde_json::json!("session_override"));

        let turn = TurnContext::new(&session);
        assert_eq!(
            turn.get("session_key"),
            Some(serde_json::json!("session_val"))
        );
        assert_eq!(
            turn.get("override_key"),
            Some(serde_json::json!("session_override"))
        );

        turn.set("turn_key", serde_json::json!("turn_val"));
        turn.set("override_key", serde_json::json!("turn_override"));

        assert_eq!(session.get("turn_key"), None);
        assert_eq!(
            session.get("override_key"),
            Some(serde_json::json!("session_override"))
        );
        assert_eq!(
            turn.get("override_key"),
            Some(serde_json::json!("turn_override"))
        );

        let op = OperationContext::new(&turn);
        assert_eq!(
            op.get("session_key"),
            Some(serde_json::json!("session_val"))
        );
        assert_eq!(op.get("turn_key"), Some(serde_json::json!("turn_val")));
        assert_eq!(
            op.get("override_key"),
            Some(serde_json::json!("turn_override"))
        );

        op.set("op_key", serde_json::json!("op_val"));
        op.set("override_key", serde_json::json!("op_override"));

        assert_eq!(turn.get("op_key"), None);
        assert_eq!(
            turn.get("override_key"),
            Some(serde_json::json!("turn_override"))
        );
        assert_eq!(
            op.get("override_key"),
            Some(serde_json::json!("op_override"))
        );
    }

    #[tokio::test]
    async fn test_hook_execution_and_dispatch() {
        let mut runner = HookRunner::new();

        let session_start_count = Arc::new(Mutex::new(0));
        let session_start_count_clone = session_start_count.clone();
        runner.register_on_session_start(move |_ctx| {
            let count = session_start_count_clone.clone();
            async move {
                let mut c = count.lock().unwrap();
                *c += 1;
                Ok(())
            }
        });

        let pre_turn_count = Arc::new(Mutex::new(0));
        let pre_turn_count_clone = pre_turn_count.clone();
        runner.register_pre_turn(move |_ctx, _prompt| {
            let count = pre_turn_count_clone.clone();
            async move {
                let mut c = count.lock().unwrap();
                *c += 1;
                Ok(HookResult::allow())
            }
        });

        let post_turn_count = Arc::new(Mutex::new(0));
        let post_turn_count_clone = post_turn_count.clone();
        runner.register_post_turn(move |_ctx, _response| {
            let count = post_turn_count_clone.clone();
            async move {
                let mut c = count.lock().unwrap();
                *c += 1;
                Ok(())
            }
        });

        // Verify has_hooks
        assert!(runner.has_hooks());

        // Dispatch session start
        runner.dispatch_session_start().await.unwrap();
        assert_eq!(*session_start_count.lock().unwrap(), 1);

        // Dispatch pre_turn
        let prompt = Content::default();
        let (res, turn_ctx) = runner.dispatch_pre_turn(&Some(prompt)).await.unwrap();
        assert!(res.allow);
        assert_eq!(*pre_turn_count.lock().unwrap(), 1);

        // Dispatch post_turn
        runner.dispatch_post_turn(&turn_ctx, "hello").await.unwrap();
        assert_eq!(*post_turn_count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_hook_short_circuiting() {
        let mut runner = HookRunner::new();

        let hook1_called = Arc::new(Mutex::new(false));
        let h1 = hook1_called.clone();
        runner.register_pre_turn(move |_ctx, _prompt| {
            let called = h1.clone();
            async move {
                *called.lock().unwrap() = true;
                Ok(HookResult::deny("denied by hook 1"))
            }
        });

        let hook2_called = Arc::new(Mutex::new(false));
        let h2 = hook2_called.clone();
        runner.register_pre_turn(move |_ctx, _prompt| {
            let called = h2.clone();
            async move {
                *called.lock().unwrap() = true;
                Ok(HookResult::allow())
            }
        });

        let prompt = Content::default();
        let (res, _turn_ctx) = runner.dispatch_pre_turn(&Some(prompt)).await.unwrap();

        assert!(!res.allow);
        assert_eq!(res.message, "denied by hook 1");
        assert!(*hook1_called.lock().unwrap());
        assert!(!*hook2_called.lock().unwrap()); // hook 2 should be short-circuited/not called
    }

    #[tokio::test]
    async fn test_policy_engine_as_hook() {
        use crate::policy::{PolicyEngine, allow_all, deny};

        let policies = vec![deny("run_command"), allow_all()];
        let engine = Arc::new(PolicyEngine::new(policies));

        let mut runner = HookRunner::new();
        // Since Arc<T> has a blanket implementation of PreToolCallDecide:
        runner.register_pre_tool_call_decide(engine);

        let turn_ctx = TurnContext::new(&runner.session_context);

        // Denied tool call
        let call_run = ToolCall {
            id: Some("1".to_string()),
            name: "run_command".to_string(),
            args: serde_json::json!({}),
            arguments_json: None,
            canonical_path: None,
        };
        let (res1, _, _) = runner
            .dispatch_pre_tool_call(&turn_ctx, &call_run)
            .await
            .unwrap();
        assert!(!res1.allow);
        assert!(res1.message.contains("Denied by policy"));

        // Allowed tool call
        let call_view = ToolCall {
            id: Some("2".to_string()),
            name: "view_file".to_string(),
            args: serde_json::json!({}),
            arguments_json: None,
            canonical_path: None,
        };
        let (res2, _, _) = runner
            .dispatch_pre_tool_call(&turn_ctx, &call_view)
            .await
            .unwrap();
        assert!(res2.allow);
    }
}
