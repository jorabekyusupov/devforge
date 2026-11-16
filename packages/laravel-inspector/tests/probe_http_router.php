<?php
// Disposable real Laravel HTTP Request + companion + synthetic local collector.
require getenv('DF_VENDOR').'/autoload.php';
require getenv('DF_COMPANION').'/Capture.php';
require getenv('DF_COMPANION').'/RequestCapture.php';

use DevForge\Inspector\Capture;
use DevForge\Inspector\RequestCapture;
use Illuminate\Config\Repository;
use Illuminate\Container\Container;
use Illuminate\Http\Request;
use Illuminate\Http\Response;

$app = new Container();
Container::setInstance($app);
$app->instance('config', new Repository(['app' => ['key' => 'synthetic-key']]));
$capture = new Capture(getenv('DF_ENDPOINT'), getenv('DF_TOKEN'), getenv('DF_PROJECT'));
$response = (new RequestCapture($capture))->handle(Request::capture(), fn () => new Response('fixture-ok'));
$capture->finish(['status' => $response->getStatusCode()]);
$response->send();
