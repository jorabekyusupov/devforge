<?php
// Actual Laravel cache repository/dispatcher, with an isolated in-memory store.
// The caller supplies an already-installed vendor directory; nothing is downloaded.
$vendor = getenv('DEVFORGE_A04_VENDOR');
if (!$vendor || !is_file($vendor.'/autoload.php')) throw new RuntimeException('offline vendor required');
require $vendor.'/autoload.php';
require __DIR__.'/../src/Capture.php';
require __DIR__.'/../src/InspectorServiceProvider.php';

use Illuminate\Cache\ArrayStore;
use Illuminate\Cache\Repository as CacheRepository;
use Illuminate\Config\Repository as ConfigRepository;
use Illuminate\Container\Container;
use Illuminate\Contracts\Http\Kernel;
use Illuminate\Events\Dispatcher;
use Illuminate\Foundation\Application;
use DevForge\Inspector\Capture;
use DevForge\Inspector\InspectorServiceProvider;

$app = new Application(sys_get_temp_dir());
Container::setInstance($app);
$app->instance('config', new ConfigRepository(['app'=>['env'=>'testing','key'=>'synthetic-key']]));
$app->detectEnvironment(fn () => 'testing');
$events = new Dispatcher($app);
$app->instance('events', $events);
$app->instance(Kernel::class, new class { public function prependMiddleware($class): void {} });
putenv('DEVFORGE_INSPECTOR_ENDPOINT=http://127.0.0.1:1/v1/events');
putenv('DEVFORGE_INSPECTOR_TOKEN='.str_repeat('a',64));
putenv('DEVFORGE_INSPECTOR_PROJECT='.str_repeat('b',32));
(new InspectorServiceProvider($app))->boot();
$capture = $app->make(Capture::class);
$cache = new CacheRepository(new ArrayStore());
$cache->setEventDispatcher($events);
$key = 'PRIVATE_CACHE_KEY_CANARY';
$value = 'PRIVATE_CACHE_VALUE_CANARY';
$cache->get($key); // no active context: no event
$capture->begin('request', 'GET /fixture');
$cache->get($key); $cache->put($key, $value, 60); $cache->get($key); $cache->forget($key);
$capture->cache('unapproved');
$eventsProperty = new ReflectionProperty(Capture::class, 'events');
$eventsRecorded = $eventsProperty->getValue($capture);
if (array_column($eventsRecorded, 'kind') !== ['cache','cache','cache','cache']) throw new RuntimeException('cache event count');
if (array_column($eventsRecorded, 'name') !== ['miss','write','hit','forget']) throw new RuntimeException('cache event operations');
foreach ($eventsRecorded as $event) {
    if ($event['data'] !== null || $event['duration_ms'] !== 0) throw new RuntimeException('cache payload is not finite');
}
$serialized = json_encode($eventsRecorded, JSON_THROW_ON_ERROR);
if (str_contains($serialized, $key) || str_contains($serialized, $value)) throw new RuntimeException('cache secret leaked');
// Suppress shutdown flushing: no network traffic or filesystem changes.
(new ReflectionProperty(Capture::class, 'scope'))->setValue($capture, null);
$eventsProperty->setValue($capture, []);
echo "Laravel cache events: miss/write/hit/forget; no keys or values\n";
