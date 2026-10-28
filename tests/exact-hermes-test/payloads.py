"""
Snapshot tool to capture exact Hermes prompt and representative tool schema definitions
"""

import os
from pathlib import Path

REAL_PROMPT_PATH = Path("D:/Side-Projects/antigravity-api-gateway/.unlazy/real_system_prompt.txt")

if REAL_PROMPT_PATH.exists():
    with open(REAL_PROMPT_PATH, "r", encoding="utf-8") as f:
        EXACT_HERMES_SYSTEM_PROMPT = f.read()
else:
    EXACT_HERMES_SYSTEM_PROMPT = "You are Hermes Agent..."

# Actual full tool definitions extracted from active session declarations
EXACT_HERMES_TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "terminal",
            "description": "Execute shell commands on Windows bash shell. Output auto-truncated.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to execute"},
                    "timeout": {"type": "integer", "description": "Timeout seconds"}
                },
                "required": ["command"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read text file with line numbers and pagination. Format: LINE_NUM|CONTENT.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path to the file to read"},
                    "offset": {"type": "integer", "default": 1},
                    "limit": {"type": "integer", "default": 2000}
                },
                "required": ["path"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "write_file",
            "description": "Write content to a file, completely replacing existing content.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path to file"},
                    "content": {"type": "string", "description": "Complete content to write"}
                },
                "required": ["path", "content"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "patch",
            "description": "Targeted find-and-replace edits in files.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path to edit"},
                    "old_string": {"type": "string", "description": "Exact text to find and replace"},
                    "new_string": {"type": "string", "description": "Changed replacement text"}
                },
                "required": ["path", "old_string", "new_string"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "search_files",
            "description": "Search file contents or find files by name via ripgrep.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern or glob pattern"},
                    "path": {"type": "string", "default": "."},
                    "target": {"type": "string", "enum": ["content", "files"], "default": "content"}
                },
                "required": ["pattern"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "web_search",
            "description": "Search the web for up to date information.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query"},
                    "limit": {"type": "integer", "default": 5}
                },
                "required": ["query"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "consultant_profile",
            "description": "Retrieve the user's consultant memory profile and stable representation.",
            "parameters": {"type": "object", "properties": {}}
        }
    },
    {
        "type": "function",
        "function": {
            "name": "consultant_history",
            "description": "Semantic query across long-term consultant memory history with verified citations.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "The search query"},
                    "limit": {"type": "integer", "default": 5}
                },
                "required": ["query"]
            }
        }
    }
]

REAL_CONVERSATION_SCENARIOS = [
    {
        "title": "Startup Strategy & Memory Recall",
        "user_turn_1": "Consultant check: What is our current focus for the Axion startup and current milestone?",
        "simulated_tool": "consultant_history",
        "tool_args": '{"query": "startup milestone tasks M1"}',
        "tool_response": "[Citation #1: M1 Task Card for Arafat Qamzawi] Active focus is on milestone M1 execution, fast feedback loops, and validating initial market demand in Yemen.",
        "user_turn_2": "Summarize the 3 primary action items based on that context."
    },
    {
        "title": "Codebase Inspection & Test Automation",
        "user_turn_1": "Inspect our gateway codebase and check if lease-engine.js has tests covering saturation.",
        "simulated_tool": "search_files",
        "tool_args": '{"pattern": "LeaseEngine", "path": "tests/unit"}',
        "tool_response": "tests/unit/m3-wp09-wp10-lease-queue.test.mjs:5: test('WP-09: LeaseEngine allocates attempt lease atomically and releases idempotently', () => {...});",
        "user_turn_2": "Great, summarize the test coverage in 2 sentences."
    },
    {
        "title": "Deployment Architecture Verification",
        "user_turn_1": "Check deploy/start-vps.sh to verify whether pre-provisioned owner security is active.",
        "simulated_tool": "read_file",
        "tool_args": '{"path": "deploy/start-vps.sh", "limit": 20}',
        "tool_response": "1|#!/usr/bin/env bash\n2|[ -f data/console/access.json ] || { printf '%s\\n' 'Owner account must be pre-configured privately'; exit 1; }",
        "user_turn_2": "Verify if the script fails closed when access.json is missing."
    }
]
