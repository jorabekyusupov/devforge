<?php
/** Synthetic Foundation/HTTP-kernel contract; never bootstrap a user project. */
declare(strict_types=1);

use DevForge\Inspector\Capture;
use DevForge\Inspector\InspectorServiceProvider;
use Illuminate\Contracts\Http\Kernel;
use Illuminate\Contracts\Queue\ShouldQueue;
use Illuminate\Foundation\Application;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Mail;
use Illuminate\Support\Facades\Route;

$root = $argv[1] ?? '';
$mode = $argv[2] ?? 'main';
if (!is_file($root.'/composer.json') || !is_file($root.'/.devforge-e10-fixture')) {
    throw new RuntimeException('isolated_fixture_required');
}
require $root.'/vendor/autoload.php';

final class CompatibilityJob implements ShouldQueue
{
    public function __construct(public bool $fail = false) {}
    public function handle(): void
    {
        DB::select('select ? as value', ['E10_BINDING_PRIVATE_CANARY']);
        Log::info('E10 job completed', ['password' => 'E10_CONTEXT_PRIVATE_CANARY']);
        dump(['marker' => 'job-dump', 'token' => 'E10_CONTEXT_PRIVATE_CANARY']);
        if ($this->fail) {
            throw new RuntimeException('token=E10_EXCEPTION_PRIVATE_CANARY');
        }
    }
}

$configuration = [
    'app' => ['name' => 'E10 synthetic fixture', 'env' => getenv('APP_ENV'), 'debug' => false,
        'url' => 'http://localhost', 'key' => 'base64:'.base64_encode(str_repeat('k', 32)),
        'cipher' => 'AES-256-CBC', 'locale' => 'en'],
    'database' => ['default' => 'sqlite', 'connections' => ['sqlite' => [
        'driver' => 'sqlite', 'database' => ':memory:', 'prefix' => '', 'foreign_key_constraints' => true,
    ]]],
    'queue' => ['default' => 'sync', 'connections' => ['sync' => ['driver' => 'sync']]],
    'logging' => ['default' => 'single', 'channels' => ['single' => [
        'driver' => 'single', 'path' => $root.'/storage/logs/compatibility.log', 'level' => 'debug',
    ]]],
    'mail' => ['default' => 'smtp', 'mailers' => ['smtp' => [
        'transport' => 'smtp', 'scheme' => 'smtp', 'host' => '127.0.0.1',
        'port' => (int) getenv('E10_SMTP_PORT'), 'username' => getenv('DEVFORGE_INSPECTOR_PROJECT'),
        'password' => getenv('E10_SMTP_PASSWORD'), 'timeout' => 5, 'auto_tls' => false,
    ]], 'from' => ['address' => 'sender@example.test', 'name' => 'E10']],
];
foreach ($configuration as $name => $data) {
    file_put_contents($root.'/config/'.$name.'.php', '<?php return '.var_export($data, true).';');
}

$app = Application::configure(basePath: $root)
    ->withRouting(using: function (): void {
        Route::get('/compatibility', function () {
            DB::select('select ? as value', ['E10_BINDING_PRIVATE_CANARY']);
            DB::select("select 'E10_SQL_PRIVATE_CANARY' as value");
            Log::warning('token=E10_LOG_PRIVATE_CANARY', ['password' => 'E10_CONTEXT_PRIVATE_CANARY']);
            dump(['marker' => 'request-dump', 'secret' => 'E10_CONTEXT_PRIVATE_CANARY']);
            app('queue')->connection('sync')->push(new CompatibilityJob());
            try {
                app('queue')->connection('sync')->push(new CompatibilityJob(true));
            } catch (RuntimeException $error) {
                if ($error->getMessage() !== 'token=E10_EXCEPTION_PRIVATE_CANARY') throw $error;
            }
            Mail::raw('E10 local-only body', function ($message): void {
                $message->to('recipient@example.test')->subject('E10 compatibility')
                    ->attachData('E10 attachment bytes', 'contract.txt', ['mime' => 'text/plain']);
            });
            return response()->json(['ok' => true]);
        });
        Route::get('/compatibility-error', fn () => throw new RuntimeException('token=E10_EXCEPTION_PRIVATE_CANARY'));
        Route::get('/compatibility-dd', function (): never {
            dd(['marker' => 'dump-exit', 'password' => 'E10_CONTEXT_PRIVATE_CANARY']);
        });
    })
    ->withMiddleware(fn ($middleware) => $middleware->use([]))
    ->withExceptions(fn ($exceptions) => null)
    ->create();
$app->register(InspectorServiceProvider::class);
$kernel = $app->make(Kernel::class);

if ($mode === 'disabled') {
    // Bootstrap through the real kernel, but execute no uninstrumented workload.
    $kernel->bootstrap();
    if ($app->bound(Capture::class)) throw new RuntimeException('production_capture_enabled');
    echo json_encode(['disabled' => true]);
    exit(0);
}
if ($mode === 'dd') {
    $request = Request::create('/compatibility-dd', 'GET');
    $kernel->handle($request); // dd exits; Capture's shutdown handler must flush.
    throw new RuntimeException('dd_did_not_exit');
}
foreach (['/compatibility' => 200, '/compatibility-error' => 500] as $uri => $expected) {
    $request = Request::create($uri, 'GET');
    $response = $kernel->handle($request);
    if ($response->getStatusCode() !== $expected) {
        throw new RuntimeException('unexpected_http_status_'.$response->getStatusCode());
    }
    if ($expected === 200 && json_decode($response->getContent(), true) !== ['ok' => true]) {
        throw new RuntimeException('unexpected_http_body');
    }
    $kernel->terminate($request, $response);
}
if (!$app->bound(Capture::class)) throw new RuntimeException('testing_capture_missing');
echo json_encode(['framework' => Application::VERSION, 'php' => PHP_VERSION,
    'checks' => ['http_kernel_200_and_500' => true, 'production_disabled' => false,
        'sqlite_query' => true, 'sync_queue_success_and_exception' => true,
        'framework_log_dump_exception' => true, 'framework_smtp_delivery' => true]], JSON_THROW_ON_ERROR);
