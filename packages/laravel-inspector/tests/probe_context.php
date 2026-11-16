<?php
// Real Laravel Request and companion middleware; no server or network is needed.
$vendor = getenv('DEVFORGE_A08_VENDOR');
if (!$vendor || !is_file($vendor.'/autoload.php')) throw new RuntimeException('offline vendor required');
require $vendor.'/autoload.php';
require __DIR__.'/../src/Capture.php';
require __DIR__.'/../src/RequestCapture.php';

use DevForge\Inspector\Capture;
use DevForge\Inspector\RequestCapture;
use Illuminate\Config\Repository;
use Illuminate\Container\Container;
use Illuminate\Http\Request;
use Illuminate\Http\Response;

$app = new Container();
Container::setInstance($app);
$app->instance('config', new Repository(['app' => ['key' => 'synthetic-key']]));
$capture = new Capture('', str_repeat('a', 64), str_repeat('b', 32));
$middleware = new RequestCapture($capture);
$scope = new ReflectionProperty(Capture::class, 'scope');
$events = new ReflectionProperty(Capture::class, 'events');
$known = str_repeat('c', 32);
$seen = [];
foreach ([$known, 'PRIVATE_HEADER_CANARY', strtoupper($known), $known."\n", str_repeat('d', 33), null] as $index => $input) {
    $headers = $input === null ? [] : ['HTTP_X_DEVFORGE_CONTEXT' => $input];
    $request = Request::create('/fixture', 'GET', server: $headers);
    $middleware->handle($request, function ($request) use ($scope, $capture, &$seen, $index, $known) {
        $id = $scope->getValue($capture)['id'];
        if (!preg_match('/\A[0-9a-f]{32}\z/D', $id)) throw new RuntimeException('invalid context');
        if (($index === 0) !== ($id === $known)) throw new RuntimeException('probe context mismatch');
        $seen[] = $id;
        // Other scopes keep fresh IDs even when given the same request candidate.
        $capture->begin('job', 'fixture-job', $known);
        if ($scope->getValue($capture)['id'] === $known) throw new RuntimeException('job accepted request context');
        $capture->finish();
        if ($scope->getValue($capture)['id'] !== $id) throw new RuntimeException('parent request context not restored');
        return new Response('ok');
    });
    $scope->setValue($capture, null);
    (new ReflectionProperty(Capture::class, 'parents'))->setValue($capture, []);
}
if (count(array_unique($seen)) !== count($seen)) throw new RuntimeException('context collision');
if (str_contains(json_encode($events->getValue($capture), JSON_THROW_ON_ERROR), 'PRIVATE_HEADER_CANARY')) throw new RuntimeException('raw header leaked');
$events->setValue($capture, []);
echo "Laravel probe request context: valid ID, invalid fallback, independent job scopes\n";
