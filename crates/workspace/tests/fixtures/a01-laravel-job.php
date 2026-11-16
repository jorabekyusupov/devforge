<?php
// Synthetic trusted Laravel test, no application or production data.
require dirname(__DIR__).'/vendor/autoload.php';
use Illuminate\Foundation\Application;
$app = new Application(dirname(__DIR__));
$app->bootstrapWith([
    \Illuminate\Foundation\Bootstrap\LoadEnvironmentVariables::class,
    \Illuminate\Foundation\Bootstrap\LoadConfiguration::class,
    \Illuminate\Foundation\Bootstrap\RegisterFacades::class,
    \Illuminate\Foundation\Bootstrap\RegisterProviders::class,
    \Illuminate\Foundation\Bootstrap\BootProviders::class,
]);
if (!$app->environment('testing') || $app['config']['database.default'] !== 'sqlite') throw new RuntimeException('normal Laravel bootstrap escaped testing');
foreach (['APP_ENV'=>'testing','DB_CONNECTION'=>'sqlite','DB_URL'=>'','DATABASE_URL'=>'',
    'APP_KEY'=>'base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=', 'CACHE_STORE'=>'array','CACHE_DRIVER'=>'array','SESSION_DRIVER'=>'array','QUEUE_CONNECTION'=>'sync','MAIL_MAILER'=>'array'] as $key=>$value) {
    if (getenv($key) !== $value) throw new RuntimeException('invalid fixed test context');
}
$database = getenv('DB_DATABASE');
if (str_contains($database, '/tree/') || basename(dirname($database)) === 'database') throw new RuntimeException('development path refused');
$migrations = dirname($database).'/migrations';
mkdir($migrations);
file_put_contents($migrations.'/2026_01_01_000000_create_a01_records.php', <<<'MIGRATION'
<?php
return new class extends \Illuminate\Database\Migrations\Migration {
    public function up(): void {
        \Illuminate\Support\Facades\Schema::create('a01_records', function (\Illuminate\Database\Schema\Blueprint $table) {
            $table->id(); $table->string('value');
        });
    }
    public function down(): void { \Illuminate\Support\Facades\Schema::dropIfExists('a01_records'); }
};
MIGRATION);
$repository = $app->make('migration.repository');
$repository->createRepository();
$app->make('migrator')->run([$migrations]);
$db = $app->make('db');
$db->table('a01_records')->insert(['value'=>'synthetic']);
if ($db->table('a01_records')->count() !== 1) throw new RuntimeException('create failed');
$db->table('a01_records')->delete();
if ($db->table('a01_records')->count() !== 0) throw new RuntimeException('delete failed');
echo 'Laravel '.Application::VERSION.' isolated migration/create/delete passed'.PHP_EOL;
