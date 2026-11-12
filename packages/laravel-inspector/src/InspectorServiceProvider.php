<?php
namespace DevForge\Inspector;
use Illuminate\Support\ServiceProvider;
final class InspectorServiceProvider extends ServiceProvider
{
    public function register(): void {}
    public function boot(): void
    {
        $endpoint = getenv('DEVFORGE_INSPECTOR_ENDPOINT') ?: '';
        $token = getenv('DEVFORGE_INSPECTOR_TOKEN') ?: '';
        $project = getenv('DEVFORGE_INSPECTOR_PROJECT') ?: '';
        if (!$this->app->environment(['local', 'testing']) || !preg_match('/^[a-f0-9]{64}$/', $token) || !preg_match('/^[a-f0-9]{32}$/', $project)) return;
        $capture = new Capture($endpoint, $token, $project);
        $this->app->instance(Capture::class, $capture);
        $this->app->make(\Illuminate\Contracts\Http\Kernel::class)->prependMiddleware(RequestCapture::class);
        $events = $this->app['events'];
        $events->listen(\Illuminate\Routing\Events\RouteMatched::class, fn ($event) => $capture->requestName($event->request->method().' '.$event->route->uri()));
        $events->listen(\Illuminate\Foundation\Http\Events\RequestHandled::class, fn ($event) => $capture->finish(['status' => $event->response->getStatusCode()]));
        $events->listen(\Illuminate\Database\Events\QueryExecuted::class, fn ($event) => $capture->query($event->sql, (float)$event->time, $event->connectionName));
        $events->listen(\Illuminate\Log\Events\MessageLogged::class, function ($event) use ($capture) {
            $exception = $event->context['exception'] ?? null;
            $message = $exception instanceof \Illuminate\Database\QueryException ? get_class($exception) : (is_string($event->message) ? $event->message : get_debug_type($event->message));
            $capture->emit('log', $event->level, 0, ['message' => $message]);
            if ($exception instanceof \Throwable) $capture->exception($exception);
        });
        $events->listen(\Illuminate\Queue\Events\JobProcessing::class, fn ($event) => $capture->begin('job', $event->job->resolveName()));
        $events->listen(\Illuminate\Queue\Events\JobProcessed::class, fn ($event) => $capture->finish(['status' => 'succeeded']));
        $events->listen(\Illuminate\Queue\Events\JobExceptionOccurred::class, function ($event) use ($capture) { $capture->exception($event->exception); $capture->finish(['status' => 'failed']); });
        $previous = null; $handler = null;
        $handler = function ($value, $label = null) use ($capture, &$previous, &$handler) {
            if ($capture->active()) { $capture->dump($value); return; }
            \Symfony\Component\VarDumper\VarDumper::setHandler($previous);
            try { \Symfony\Component\VarDumper\VarDumper::dump($value, $label); }
            finally { \Symfony\Component\VarDumper\VarDumper::setHandler($handler); }
        };
        $previous = \Symfony\Component\VarDumper\VarDumper::setHandler($handler);
    }
}
