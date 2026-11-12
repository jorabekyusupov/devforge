# DevForge Laravel Inspector companion (internal)

This opt-in development package emits project-scoped request/job/query/log/dump
and exception events to the DevForge desktop collector. It does not execute SQL,
add a PHP extension, or send telemetry to an external service.

Use the desktop **Inspector → Install companion** confirmation in a trusted local
Laravel project. Stop that project first. DevForge writes its owned companion source to
`.devforge/laravel-inspector`, adds a project-local Composer path repository,
requires/reinstalls `devforge/laravel-inspector` as a development dependency with
scripts/plugins disabled, then explicitly runs `artisan package:discover`.
`composer.json`, `composer.lock`, `vendor` and the integration directory change;
`.env` does not. Composer dependency failures are actionable retries, not a promise
that arbitrary application dependencies are compatible. Existing conflicting
integration files are preserved rather than overwritten.

After installation, choose **Enable capture**. DevForge restarts only the selected
running project so its PHP web/queue processes receive the private collector
session. The companion only activates for Laravel `local`/`testing` environments.
Open a route, execute queries, dump data, or dispatch a job. Use the event filter,
search and context profile to inspect results. **Pause capture** restarts that
project without capture credentials; **Clear events** removes only its history.

The package requires PHP8.3+ and declares Laravel11/12/13 compatibility. Actual
native integration evidence currently covers the Laravel13 fixture; declarations
are not verification of all applications/framework versions. A project excluding
package auto-discovery may need explicit provider registration in its own
bootstrap configuration. Do not automatically edit that application's provider
policy to bypass its choice.

Request bodies/headers, query bindings and queue payloads are not collected.
SQL literals/comments and known credential values/sensitive fields are redacted.
Objects expose bounded public properties without invoking user serializers;
output is JSON/text, never captured HTML. Redaction is not a guarantee for unknown
or encoded secrets. Use local test data and review records before sharing them.

`dump()` values in an active captured scope go to Inspector. `dd()` retains its
exit behavior, with a shutdown context record. Outside captured scopes or after
the transport circuit is marked offline, the prior Symfony dump handler is
restored for the call. The first dump detecting a transport failure may be lost.
A bounded transport/circuit breaker avoids repeated connection waits in an
offline context. One final request/job summary is attempted within the existing
transport deadlines and marks incomplete capture. Events remain best-effort and
may be lost or truncated under limits; this is not a lossless tracing system.

Profiles report instrumented request/job elapsed time, captured SQL count/time,
normalized repeated/slow query hints and PHP peak memory. They include diagnostic
overhead. Repeated queries suggest potential N+1, not proof. This is not a
function-call/CPU profiler. **Check selected PHP** detects Xdebug version/mode;
it does not install an extension or claim unavailable debugger support.

Capture is session-scoped and is off after desktop restart. Visible history covers
up to24h/latest200 events; retained records have a4000-event/16MiB payload budget
and are pruned on ingestion. This is internal tooling; no public package release
or project license decision is implied. Remove the development dependency through
the project's normal Composer workflow when no longer needed.
