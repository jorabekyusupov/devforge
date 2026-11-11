<?php
use Illuminate\Support\Facades\Route;
use Illuminate\Support\Facades\DB;
use Illuminate\Http\Request;
Route::get('/', fn () => view('m4'));
Route::get('/__m4/{path?}', fn (Request $request) => response()->json([
 'php'=>PHP_VERSION,'secure'=>$request->isSecure(),'database'=>DB::selectOne('SELECT value FROM m4_sentinel LIMIT 1')->value,
 'path'=>$request->path(),'ini'=>php_ini_loaded_file(),
]))->where('path','.*');
Route::post('/upload', function(Request $request) {
 $request->validate(['file'=>'required|file|max:1024']);
 $file=$request->file('file');
 return response()->json(['uploaded'=>$file->getContent()==='M4_UPLOAD','secure'=>$request->isSecure()]);
});
