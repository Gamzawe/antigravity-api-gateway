"""
Real Hermes Agent Payload Templates
Contains actual system prompts, tool schemas, and simulated tool outputs
representing a realistic Hermes agent session (~4k - 8k tokens).
"""

HERMES_SYSTEM_PROMPT = """You are Hermes Agent, built by Nous Research. Be direct: match the length of your reply to the weight of the ask.
You are equipped with tools for terminal execution, file manipulation, and web retrieval.
Available tools:
- web_search(query: str, limit: int) -> dict
- read_file(path: str, offset: int, limit: int) -> dict
- search_files(pattern: str, path: str) -> dict
- write_file(path: str, content: str) -> dict
- terminal(command: str) -> dict

Always verify before answering and ground your findings in real tool outputs."""

HERMES_TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "web_search",
            "description": "Search the web for real-time information",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search keywords"}
                },
                "required": ["query"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read file contents with line numbers",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path to file"}
                },
                "required": ["path"]
            }
        }
    },
    {
        "type": "function",
        "function": {
            "name": "search_files",
            "description": "Search file contents using regex",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern"}
                },
                "required": ["pattern"]
            }
        }
    }
]

USER_SCENARIOS = [
    {
        "task": "Check the project configuration in config.yaml and list all active integrations.",
        "simulated_tool": "read_file",
        "tool_args": '{"path": "config.yaml"}',
        "tool_result": "active_integrations:\n  - telegram\n  - memory_system\n  - private_gateway\n  - mcp_windows\nport: 8089\nstatus: active"
    },
    {
        "task": "Search for any rate limit definitions in the source code.",
        "simulated_tool": "search_files",
        "tool_args": '{"pattern": "DEFAULT_COOLDOWN_MS"}',
        "tool_result": "src/constants.js:15: export const DEFAULT_COOLDOWN_MS = 2000;\nsrc/account-manager/rate-limits.js:8: import { DEFAULT_COOLDOWN_MS } from '../constants.js';"
    },
    {
        "task": "Search recent docs regarding Antigravity CloudCode quota handling.",
        "simulated_tool": "web_search",
        "tool_args": '{"query": "Antigravity CloudCode quota resetDelay"}',
        "tool_result": "Google CloudCode returns HTTP 429 with quotaResetDelay metadata between 100ms and 2000ms for short burst throttling."
    }
]
