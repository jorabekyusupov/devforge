<?php
namespace DevForge\Inspector;
final class RequestCapture
{
    public function __construct(private Capture $capture) {}
    public function handle($request, \Closure $next)
    {
        $this->capture->begin('request', $request->method().' [unmatched route]');
        $status = 500;
        try { $response = $next($request); $status = $response->getStatusCode(); return $response; }
        catch (\Throwable $error) {
            $this->capture->exception($error);
            throw $error;
        } finally {
            $this->capture->requestName($request->method().' '.($request->route()?->uri() ?? '[unmatched route]'));

        }
    }
}
