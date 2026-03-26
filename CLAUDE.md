# Agent Tab — Rust + Infra

> Scope: agent/, helper/, infra/. For full project context see /z/nucleus-portal/CLAUDE.md

## Rust Agent (agent/)
- **nucleus-agent**: Main binary — WebSocket client to backend, health metrics via sysinfo
- **nucleus-common**: Shared types (messages.rs, types.rs)

### Build
```bash
cd /z/nucleus-portal/agent
cargo build                        # debug
cargo build --release              # release (opt-level z, LTO, strip)
bash ../scripts/build-agent.sh     # cross-compile for ARM target
```

### Run
```bash
cargo run -- --config agent.example.toml
```

### Source
```
agent/
├── nucleus-agent/src/
│   ├── main.rs         Entry + tokio runtime
│   ├── config.rs       TOML config loading
│   ├── connection.rs   WebSocket connection + reconnect loop
│   └── health.rs       CPU/mem/disk metrics (sysinfo)
└── nucleus-common/src/
    ├── lib.rs          Module exports
    ├── messages.rs     WS message types (AgentMessage, ServerMessage)
    └── types.rs        Common types (DeviceInfo, HealthMetrics)
```

### Key Conventions
- tokio async — all IO is async/await
- WebSocket protocol defined in `nucleus-common/src/messages.rs`
- Must match backend `agent-gateway/` expectations
- Config via TOML (`agent.example.toml`) — never hardcode credentials
- Reconnect with exponential backoff on connection drop

## Rust Helper (helper/)
- **nucleus-helper**: Desktop client — handles tunnel connections for support sessions

### Build
```bash
cd /z/nucleus-portal/helper
cargo build --release
```

## Infrastructure (infra/)

### Start
```bash
cd /z/nucleus-portal
bash scripts/dev.sh           # starts all infra via docker compose
```

### Stop
```bash
docker compose -f infra/docker-compose.yml down
```

### Services
| Service    | Port | Image                          |
|------------|------|--------------------------------|
| Postgres   | 5432 | timescale/timescaledb:pg16     |
| PgBouncer  | 6432 | pgbouncer/pgbouncer            |
| Redis      | 6379 | redis:7-alpine                 |

### DB Init
- Schema + seed: `infra/db/init.sql` (auto-run on first Postgres start)
- Drizzle migrations: `pnpm db:migrate` from project root

### Docker Builds
```bash
docker build -f infra/docker/Dockerfile.backend -t nucleus-backend .
docker build -f infra/docker/Dockerfile.frontend -t nucleus-frontend .
```
