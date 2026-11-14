# Private manager CLI — experimental standalone mode

The current private manager entry is an integration surface, not a public MCP
server or the completed JSON automation API. The legacy desktop still owns its
own Workspace; close that desktop before starting this standalone manager. The
manager deliberately holds the legacy GUI exclusion lease until client migration.

```sh
devforge manager --help
devforge manager health
devforge manager serve
```

`serve` is an explicit operator start and clears a reviewed owner inhibition state.
Automation must use `manager serve --automatic`: it refuses to restart an owner
after explicit Quit. Neither ordinary serve mode nor health prints a capability.
Health is a sanitized readiness response; it does not expose projects or secrets.
The native credential stores are constructed but are not queried at startup.

A valid authenticated private Quit, or Ctrl+C on the foreground owner, persists
inhibition before stopping acceptance and joining Workspace cleanup. If saving
inhibition fails, the owner reports a stable error and remains alive. A forced OS
kill is a crash, not an acknowledged graceful Quit. Restart receipts do not replay
old commands; interrupted/running work needs reconciliation.

Existing legacy foreground/mutating CLI routes take a manager-only exclusion
lease. They cannot open another owner beside an active manager. They keep their
own original operation leases, including exclusive config setup/secret, so the
compatibility guard does not self-deadlock. Read-only doctor, retained Inspector,
runtime catalog and project list/inspect remain available. M13.2 will forward the
other supported commands through the owner; the current guard is transitional.

The private native GUI launch mode is reserved for a verified bundled parent and
an actual inherited pipe. It never emits a token to a terminal, file or null.
It is not a general operator/API credential export command and is not advertised
in help. Parent launch/readiness/session behavior is specified in the
[bootstrap ADR](../architecture/manager-bootstrap.md).

Current gates: no GUI client migration, no external project grant tools, no public
MCP, and no complete old UI feature compatibility. In particular, the default
64KiB private frame and current actor command limit cannot yet carry all existing
M7 request bodies. Do not replace the desktop route before authenticated large
payload and trusted target adapters are accepted.

The CLI test uses isolated data, actual compiled CLI child + private pipe,
root/PID-pinned IPC, one-use redemption, public health and orderly Quit. The next
automatic serve is refused. No native credential API, existing user project,
DNS/trust or global configuration is changed. Native end-to-end evidence is macOS
ARM64; other target execution is tracked separately in the execution ledger.
