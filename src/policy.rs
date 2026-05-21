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

//! Declaring and enforcing safety policies for tool execution.

use crate::types::{BuiltinTools, ToolCall};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Deny,
    AskUser,
}

#[derive(Clone)]
pub struct Policy {
    pub tool: String,
    pub decision: Decision,
    pub when: Option<Arc<dyn Fn(&serde_json::Value) -> bool + Send + Sync>>,
    pub ask_user: Option<Arc<dyn Fn(&ToolCall) -> bool + Send + Sync>>,
    pub name: String,
}

impl Policy {
    pub fn new(tool: &str, decision: Decision, name: &str) -> Self {
        Self {
            tool: tool.to_string(),
            decision,
            when: None,
            ask_user: None,
            name: name.to_string(),
        }
    }

    pub fn when<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&serde_json::Value) -> bool + Send + Sync + 'static,
    {
        self.when = Some(Arc::new(predicate));
        self
    }

    pub fn name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }
}

pub struct PolicyEngine {
    buckets: Vec<Vec<Policy>>,
}

impl PolicyEngine {
    pub fn new(policies: Vec<Policy>) -> Self {
        let mut buckets = vec![Vec::new(); 6];
        for p in policies {
            let is_wildcard = p.tool == "*";
            let idx = match (is_wildcard, p.decision) {
                (false, Decision::Deny) => 0,
                (false, Decision::AskUser) => 1,
                (false, Decision::Approve) => 2,
                (true, Decision::Deny) => 3,
                (true, Decision::AskUser) => 4,
                (true, Decision::Approve) => 5,
            };
            buckets[idx].push(p);
        }
        Self { buckets }
    }

    pub fn evaluate(&self, tool_call: &ToolCall) -> Result<bool, String> {
        for bucket in &self.buckets {
            for policy in bucket {
                if policy.tool == "*" || policy.tool == tool_call.name {
                    let predicate_matches = if let Some(ref when_fn) = policy.when {
                        when_fn(&tool_call.args)
                    } else {
                        true
                    };

                    if predicate_matches {
                        match policy.decision {
                            Decision::Approve => return Ok(true),
                            Decision::Deny => {
                                return Err(format!("Denied by policy '{}'", policy.name));
                            }
                            Decision::AskUser => {
                                if let Some(ref ask_fn) = policy.ask_user {
                                    let approved = ask_fn(tool_call);
                                    if approved {
                                        return Ok(true);
                                    } else {
                                        return Err(format!(
                                            "User denied tool '{}' (policy '{}')",
                                            tool_call.name, policy.name
                                        ));
                                    }
                                } else {
                                    return Err(format!(
                                        "ASK_USER policy '{}' is missing a handler",
                                        policy.name
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(true)
    }
}

// =============================================================================
// Helper Creators
// =============================================================================

pub fn allow(tool: &str) -> Policy {
    Policy::new(tool, Decision::Approve, "allow")
}

pub fn deny(tool: &str) -> Policy {
    Policy::new(tool, Decision::Deny, "deny")
}

pub fn ask_user<F>(tool: &str, handler: F) -> Policy
where
    F: Fn(&ToolCall) -> bool + Send + Sync + 'static,
{
    let mut p = Policy::new(tool, Decision::AskUser, "ask_user");
    p.ask_user = Some(Arc::new(handler));
    p
}

pub fn allow_all() -> Policy {
    Policy::new("*", Decision::Approve, "allow_all")
}

pub fn deny_all() -> Policy {
    Policy::new("*", Decision::Deny, "deny_all")
}

pub fn confirm_run_command(
    handler: Option<Arc<dyn Fn(&ToolCall) -> bool + Send + Sync>>,
) -> Vec<Policy> {
    let mut policies = Vec::new();
    if let Some(h) = handler {
        let mut p = Policy::new(
            BuiltinTools::RunCommand.as_str(),
            Decision::AskUser,
            "confirm_run_command",
        );
        p.ask_user = Some(h);
        policies.push(p);
    } else {
        policies.push(deny(BuiltinTools::RunCommand.as_str()));
    }
    policies.push(allow_all());
    policies
}

// =============================================================================
// Path containment verification
// =============================================================================

pub fn is_path_in_workspace<P1: AsRef<Path>, P2: AsRef<Path>>(target: P1, workspace: P2) -> bool {
    let target = target.as_ref();
    let workspace = workspace.as_ref();

    let target_abs = if target.is_absolute() {
        clean_path(target)
    } else if let Ok(curr) = std::env::current_dir() {
        clean_path(&curr.join(target))
    } else {
        clean_path(target)
    };

    let ws_abs = if workspace.is_absolute() {
        clean_path(workspace)
    } else if let Ok(curr) = std::env::current_dir() {
        clean_path(&curr.join(workspace))
    } else {
        clean_path(workspace)
    };

    let is_case_insensitive = cfg!(target_os = "windows") || cfg!(target_os = "macos");

    let t_components: Vec<_> = target_abs.components().collect();
    let w_components: Vec<_> = ws_abs.components().collect();

    if t_components.len() < w_components.len() {
        return false;
    }

    for (t, w) in t_components.iter().zip(w_components.iter()) {
        let t_str = t.as_os_str().to_string_lossy();
        let w_str = w.as_os_str().to_string_lossy();

        if is_case_insensitive {
            if t_str.to_lowercase() != w_str.to_lowercase() {
                return false;
            }
        } else if t_str != w_str {
            return false;
        }
    }

    true
}

fn clean_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

pub fn workspace_only(workspaces: Vec<String>) -> Vec<Policy> {
    let mut policies = Vec::new();
    let workspaces_arc = Arc::new(workspaces);

    let file_tools = vec![
        BuiltinTools::ViewFile.as_str().to_string(),
        BuiltinTools::CreateFile.as_str().to_string(),
        BuiltinTools::EditFile.as_str().to_string(),
    ];

    for tool in file_tools {
        let w_clone = workspaces_arc.clone();
        let mut policy = Policy::new(&tool, Decision::Deny, "workspace_only");
        policy.when = Some(Arc::new(move |args| {
            let mut path_opt = None;
            for key in &[
                "canonical_path",
                "path",
                "file_path",
                "TargetFile",
                "directory_path",
            ] {
                if let Some(val) = args.get(*key) {
                    if let Some(s) = val.as_str() {
                        path_opt = Some(s.to_string());
                        break;
                    }
                }
            }

            if let Some(path) = path_opt {
                if path.is_empty() {
                    return false;
                }
                !w_clone.iter().any(|ws| is_path_in_workspace(&path, ws))
            } else {
                false
            }
        }));
        policies.push(policy);
    }

    policies
}

impl crate::hooks::PreToolCallDecide for PolicyEngine {
    fn run<'a>(
        &'a self,
        _context: &'a crate::hooks::OperationContext,
        tool_call: &'a ToolCall,
    ) -> crate::hooks::HookFuture<'a, Result<crate::hooks::HookResult, String>> {
        let res = match self.evaluate(tool_call) {
            Ok(true) => Ok(crate::hooks::HookResult::allow()),
            Ok(false) => Ok(crate::hooks::HookResult::deny("")),
            Err(err) => Ok(crate::hooks::HookResult::deny(&err)),
        };
        Box::pin(async move { res })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_is_path_in_workspace() {
        assert!(is_path_in_workspace("/a/b/c", "/a/b"));
        assert!(is_path_in_workspace("/a/b/c/d.txt", "/a/b"));
        assert!(!is_path_in_workspace("/a/x/c", "/a/b"));
    }

    #[test]
    fn test_policy_engine() {
        let policies = vec![deny("run_command"), allow_all()];
        let engine = PolicyEngine::new(policies);

        let call_run = ToolCall {
            id: Some("1".to_string()),
            name: "run_command".to_string(),
            args: json!({}),
            arguments_json: None,
            canonical_path: None,
        };
        assert!(engine.evaluate(&call_run).is_err());

        let call_view = ToolCall {
            id: Some("2".to_string()),
            name: "view_file".to_string(),
            args: json!({}),
            arguments_json: None,
            canonical_path: None,
        };
        assert!(engine.evaluate(&call_view).is_ok());
    }
}
