<?php
namespace DevForge\Inspector;

/** Bounded local telemetry. No request bodies, headers, SQL bindings or job payloads. */
final class Capture
{
    private ?array $scope = null;
    private array $events = [];
    private array $secrets = [];
    private bool $sending = false;
    private int $count = 0;
    private bool $offline = false;
    private array $parents = [];
    public function __construct(private string $endpoint, private string $token, private string $project)
    {
        $this->secrets = array_values(array_filter([$token, config('app.key'), config('database.connections.pgsql.password'), config('database.connections.mysql.password')]));
        register_shutdown_function(function (): void {
            while ($this->scope !== null) $this->finish(['interrupted' => true]);
            $this->flush();
        });
    }
    public function begin(string $kind, string $name): void
    {
        if ($this->scope !== null) $this->parents[] = [$this->scope, $this->count, $this->offline];
        $this->scope = ['id' => bin2hex(random_bytes(16)), 'kind' => $kind, 'name' => $name, 'start' => hrtime(true), 'queries' => 0, 'sql_ms' => 0.0, 'exceptions' => []];
        $this->count = 0; $this->offline = false;
    }
    public function active(): bool { return $this->scope !== null && !$this->offline; }
    public function requestName(string $name): void { if ($this->scope !== null) $this->scope['name'] = $name; }
    public function finish(array $data = []): void
    {
        if ($this->scope === null) return;
        $scope = $this->scope;
        // A late acknowledgement may trip the circuit after events were accepted.
        // Preserve one bounded final-summary attempt and disclose incomplete capture.
        $incomplete = $this->offline;
        $this->offline = false;
        $this->emit($scope['kind'], $scope['name'], (hrtime(true) - $scope['start']) / 1e6, array_merge($data, ['query_count' => $scope['queries'], 'sql_ms' => round($scope['sql_ms'], 3), 'peak_memory_bytes' => memory_get_peak_usage(true), 'truncated' => $incomplete || $this->count >= 500]), true);
        $this->flush(); $this->scope = null;
        if ($this->parents !== []) [$this->scope, $this->count, $this->offline] = array_pop($this->parents);
    }
    public function query(string $sql, float $milliseconds, string $connection): void
    {
        if ($this->scope === null || $this->sending) return;
        $this->scope['queries']++; $this->scope['sql_ms'] += $milliseconds;
        // Database listeners receive parameterized SQL. Never call toRawSql or include bindings.
        $sql = preg_replace('/\'(?:\'\'|\\\\.|[^\'])*\'|--[^\n]*|\/\*.*?\*\//s', '?', $sql) ?? '[SQL]';
        $this->emit('query', $sql, $milliseconds, ['connection' => $connection]);
    }
    public function emit(string $kind, string $name, float $milliseconds = 0, mixed $data = null, bool $final = false): void
    {
        if ($this->scope === null || $this->sending || $this->offline || (!$final && $this->count >= 500)) return;
        $this->count++;
        $this->events[] = ['kind' => $kind, 'context' => $this->scope['id'], 'name' => $this->cleanString($name), 'duration_ms' => max(0, $milliseconds), 'data' => $this->clean($data)];
        if (count($this->events) >= 20 || $kind === 'dump') $this->flush();
    }
    public function exception(\Throwable $error): void
    {
        if ($this->scope === null || isset($this->scope['exceptions'][spl_object_id($error)])) return;
        $this->scope['exceptions'][spl_object_id($error)] = true;
        $message = $error instanceof \Illuminate\Database\QueryException ? 'Database query failed: '.$error->getCode() : $error->getMessage();
        $this->emit('exception', get_class($error), 0, ['message' => $message, 'trace' => array_slice(array_map(fn ($frame) => ['file' => isset($frame['file']) ? basename($frame['file']) : '', 'line' => $frame['line'] ?? 0, 'function' => ($frame['class'] ?? '').($frame['function'] ?? '')], $error->getTrace()), 0, 15)]);
    }
    public function dump(mixed $value): void { $this->emit('dump', get_debug_type($value), 0, $this->clean($value)); }
    private function cleanString(string $value): string
    {
        $value = str_replace($this->secrets, '[REDACTED]', $value);
        $value = preg_replace('/(password|secret|token|authorization|api[_-]?key|app[_-]?key)\s*[:=]\s*[^\s,;]+/i', '$1=[REDACTED]', $value) ?? '[unavailable]';
        return mb_strcut($value, 0, 1800, 'UTF-8');
    }
    private function clean(mixed $value, int $depth = 0): mixed
    {
        if ($depth >= 5) return '[depth limit]';
        if (is_string($value)) return $this->cleanString($value);
        if (is_scalar($value) || $value === null) return $value;
        if (is_object($value)) return ['class' => get_class($value), 'properties' => $this->clean(get_object_vars($value), $depth + 1)];
        if (is_resource($value)) return '[resource]';
        $out = []; $n = 0;
        foreach ($value as $key => $entry) {
            if ($n++ >= 30) { $out['...'] = '[item limit]'; break; }
            $out[$key] = preg_match('/password|passwd|secret|token|authorization|cookie|credential|bindings|headers|body|environment|api.?key|app.?key/i', (string)$key) ? '[REDACTED]' : $this->clean($entry, $depth + 1);
        }
        return $out;
    }
    private function flush(): void
    {
        if ($this->events === [] || $this->sending) return;
        $events = $this->events; $this->events = []; $this->sending = true;
        try {
            // Failed instrumentation must not fail the user's request/job.
            foreach (array_chunk($events, 10) as $chunk) {
                $body = json_encode(['schema' => 1, 'project_id' => $this->project, 'id' => bin2hex(random_bytes(16)), 'events' => $chunk], JSON_INVALID_UTF8_SUBSTITUTE | JSON_PARTIAL_OUTPUT_ON_ERROR);
                if (!is_string($body) || strlen($body) > 120000) continue;
                $url = parse_url($this->endpoint); $port = (int)($url['port'] ?? 0);
                if (($url['scheme'] ?? '') !== 'http' || ($url['host'] ?? '') !== '127.0.0.1' || $port < 1 || ($url['path'] ?? '') !== '/v1/events') continue;
                $stream = @stream_socket_client('tcp://127.0.0.1:'.$port, $errorCode, $errorText, .03);
                if ($stream === false) { $this->offline = true; break; }
                stream_set_timeout($stream, 0, 30000);
                $request = "POST /v1/events HTTP/1.1\r\nHost: 127.0.0.1:$port\r\nContent-Type: application/json\r\nX-DevForge-Token: {$this->token}\r\nContent-Length: ".strlen($body)."\r\nConnection: close\r\n\r\n".$body;
                $offset = 0; $deadline = microtime(true) + .05;
                while ($offset < strlen($request) && microtime(true) < $deadline) { $written = @fwrite($stream, substr($request, $offset)); if (!$written) break; $offset += $written; }
                $status = @fgets($stream, 128); fclose($stream); if (!is_string($status) || !str_contains($status, " 202 ")) { $this->offline = true; break; }
            }
        } catch (\Throwable) { /* local, best-effort diagnostics */ }
        finally { $this->sending = false; }
    }
}
