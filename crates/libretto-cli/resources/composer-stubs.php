<?php
/**
 * Libretto Composer Compatibility Stubs
 *
 * This file provides a complete implementation of Composer's Event API
 * to ensure full compatibility with Composer scripts and plugins.
 *
 * It is only loaded when the real composer/composer package is not available.
 *
 * @package Libretto
 * @license MIT
 */

namespace Composer\IO {

    /**
     * Verbosity level constants
     */
    interface IOInterface
    {
        public const QUIET = 16;
        public const NORMAL = 32;
        public const VERBOSE = 64;
        public const VERY_VERBOSE = 128;
        public const DEBUG = 256;

        public function isInteractive(): bool;
        public function isVerbose(): bool;
        public function isVeryVerbose(): bool;
        public function isDebug(): bool;
        public function isDecorated(): bool;

        /**
         * @param string|string[] $messages
         */
        public function write($messages, bool $newline = true, int $verbosity = self::NORMAL): void;

        /**
         * @param string|string[] $messages
         */
        public function writeError($messages, bool $newline = true, int $verbosity = self::NORMAL): void;

        /**
         * @param string|string[] $messages
         */
        public function writeRaw($messages, bool $newline = true, int $verbosity = self::NORMAL): void;

        /**
         * @param string|string[] $messages
         */
        public function writeErrorRaw($messages, bool $newline = true, int $verbosity = self::NORMAL): void;

        /**
         * @param string|string[] $messages
         */
        public function overwrite($messages, bool $newline = true, ?int $size = null, int $verbosity = self::NORMAL): void;

        /**
         * @param string|string[] $messages
         */
        public function overwriteError($messages, bool $newline = true, ?int $size = null, int $verbosity = self::NORMAL): void;

        /**
         * @return mixed
         */
        public function ask(string $question, $default = null);

        public function askConfirmation(string $question, bool $default = true): bool;

        /**
         * @return mixed
         */
        public function askAndValidate(string $question, callable $validator, ?int $attempts = null, $default = null);

        public function askAndHideAnswer(string $question): ?string;

        /**
         * @param array<string> $choices
         * @param int|string|bool $default
         * @return int|string|list<string>|bool
         */
        public function select(string $question, array $choices, $default, $attempts = false, string $errorMessage = 'Value "%s" is invalid', bool $multiselect = false);

        public function setAuthentication(string $repositoryName, string $username, ?string $password = null): void;

        /**
         * @return array{username: string|null, password: string|null}
         */
        public function getAuthentication(string $repositoryName): array;

        /**
         * @return array<string, array{username: string|null, password: string|null}>
         */
        public function getAuthentications(): array;

        public function hasAuthentication(string $repositoryName): bool;
    }

    /**
     * Null IO implementation - outputs nothing
     */
    class NullIO implements IOInterface
    {
        public function isInteractive(): bool { return false; }
        public function isVerbose(): bool { return false; }
        public function isVeryVerbose(): bool { return false; }
        public function isDebug(): bool { return false; }
        public function isDecorated(): bool { return false; }
        public function write($messages, bool $newline = true, int $verbosity = self::NORMAL): void {}
        public function writeError($messages, bool $newline = true, int $verbosity = self::NORMAL): void {}
        public function writeRaw($messages, bool $newline = true, int $verbosity = self::NORMAL): void {}
        public function writeErrorRaw($messages, bool $newline = true, int $verbosity = self::NORMAL): void {}
        public function overwrite($messages, bool $newline = true, ?int $size = null, int $verbosity = self::NORMAL): void {}
        public function overwriteError($messages, bool $newline = true, ?int $size = null, int $verbosity = self::NORMAL): void {}
        public function ask(string $question, $default = null) { return $default; }
        public function askConfirmation(string $question, bool $default = true): bool { return $default; }
        public function askAndValidate(string $question, callable $validator, ?int $attempts = null, $default = null) { return $default; }
        public function askAndHideAnswer(string $question): ?string { return null; }
        public function select(string $question, array $choices, $default, $attempts = false, string $errorMessage = 'Value "%s" is invalid', bool $multiselect = false) { return $default; }
        public function setAuthentication(string $repositoryName, string $username, ?string $password = null): void {}
        public function getAuthentication(string $repositoryName): array { return ['username' => null, 'password' => null]; }
        public function getAuthentications(): array { return []; }
        public function hasAuthentication(string $repositoryName): bool { return false; }
    }

    /**
     * Console IO implementation - outputs to stdout/stderr
     */
    class ConsoleIO implements IOInterface
    {
        private bool $interactive;
        private int $verbosity;
        private bool $decorated;
        private array $authentications = [];

        public function __construct(bool $interactive = true, int $verbosity = self::NORMAL, bool $decorated = true)
        {
            $this->interactive = $interactive;
            $this->verbosity = $verbosity;
            $this->decorated = $decorated;
        }

        public function isInteractive(): bool { return $this->interactive; }
        public function isVerbose(): bool { return $this->verbosity >= self::VERBOSE; }
        public function isVeryVerbose(): bool { return $this->verbosity >= self::VERY_VERBOSE; }
        public function isDebug(): bool { return $this->verbosity >= self::DEBUG; }
        public function isDecorated(): bool { return $this->decorated; }

        public function write($messages, bool $newline = true, int $verbosity = self::NORMAL): void
        {
            if ($verbosity > $this->verbosity) return;
            $messages = is_array($messages) ? $messages : [$messages];
            foreach ($messages as $message) {
                echo $message . ($newline ? "\n" : '');
            }
        }

        public function writeError($messages, bool $newline = true, int $verbosity = self::NORMAL): void
        {
            if ($verbosity > $this->verbosity) return;
            $messages = is_array($messages) ? $messages : [$messages];
            foreach ($messages as $message) {
                fwrite(STDERR, $message . ($newline ? "\n" : ''));
            }
        }

        public function writeRaw($messages, bool $newline = true, int $verbosity = self::NORMAL): void
        {
            $this->write($messages, $newline, $verbosity);
        }

        public function writeErrorRaw($messages, bool $newline = true, int $verbosity = self::NORMAL): void
        {
            $this->writeError($messages, $newline, $verbosity);
        }

        public function overwrite($messages, bool $newline = true, ?int $size = null, int $verbosity = self::NORMAL): void
        {
            $this->write($messages, $newline, $verbosity);
        }

        public function overwriteError($messages, bool $newline = true, ?int $size = null, int $verbosity = self::NORMAL): void
        {
            $this->writeError($messages, $newline, $verbosity);
        }

        public function ask(string $question, $default = null)
        {
            if (!$this->interactive) return $default;
            $this->write($question . ' ', false);
            $answer = trim(fgets(STDIN) ?: '');
            return $answer !== '' ? $answer : $default;
        }

        public function askConfirmation(string $question, bool $default = true): bool
        {
            if (!$this->interactive) return $default;
            $answer = $this->ask($question . ' [' . ($default ? 'Y/n' : 'y/N') . '] ');
            if ($answer === null || $answer === '') return $default;
            return in_array(strtolower($answer), ['y', 'yes', '1', 'true'], true);
        }

        public function askAndValidate(string $question, callable $validator, ?int $attempts = null, $default = null)
        {
            if (!$this->interactive) return $default;
            $answer = $this->ask($question, $default);
            return $validator($answer);
        }

        public function askAndHideAnswer(string $question): ?string
        {
            if (!$this->interactive) return null;
            return $this->ask($question);
        }

        public function select(string $question, array $choices, $default, $attempts = false, string $errorMessage = 'Value "%s" is invalid', bool $multiselect = false)
        {
            if (!$this->interactive) return $default;
            foreach ($choices as $key => $choice) {
                $this->write("  [$key] $choice");
            }
            return $this->ask($question, $default);
        }

        public function setAuthentication(string $repositoryName, string $username, ?string $password = null): void
        {
            $this->authentications[$repositoryName] = ['username' => $username, 'password' => $password];
        }

        public function getAuthentication(string $repositoryName): array
        {
            return $this->authentications[$repositoryName] ?? ['username' => null, 'password' => null];
        }

        public function getAuthentications(): array { return $this->authentications; }
        public function hasAuthentication(string $repositoryName): bool { return isset($this->authentications[$repositoryName]); }
    }
}

namespace Composer\Config {

    interface ConfigSourceInterface
    {
        public function addConfigSetting(string $name, $value): void;
        public function removeConfigSetting(string $name): void;
        public function addProperty(string $name, $value): void;
        public function removeProperty(string $name): void;
        public function addLink(string $type, string $name, string $value): void;
        public function removeLink(string $type, string $name): void;
        public function getName(): string;
    }
}

namespace Composer {

    use Composer\Config\ConfigSourceInterface;

    /**
     * Composer Config implementation
     */
    class Config
    {
        public const RELATIVE_PATHS = 1;
        public const SOURCE_UNKNOWN = 'unknown';

        private string $baseDir;
        private array $config;
        private ?ConfigSourceInterface $configSource = null;
        private ?ConfigSourceInterface $authConfigSource = null;
        private array $repositories = [];

        private static array $defaultConfig = [
            'vendor-dir' => 'vendor',
            'bin-dir' => '{$vendor-dir}/bin',
            'data-dir' => '',
            'cache-dir' => '',
            'cache-files-dir' => '{$cache-dir}/files',
            'cache-repo-dir' => '{$cache-dir}/repo',
            'cache-vcs-dir' => '{$cache-dir}/vcs',
            'cache-ttl' => 15552000,
            'cache-files-ttl' => null,
            'cache-files-maxsize' => '300MiB',
            'cache-read-only' => false,
            'bin-compat' => 'auto',
            'discard-changes' => false,
            'autoloader-suffix' => null,
            'sort-packages' => false,
            'optimize-autoloader' => false,
            'classmap-authoritative' => false,
            'apcu-autoloader' => false,
            'prepend-autoloader' => true,
            'github-domains' => ['github.com'],
            'github-expose-hostname' => true,
            'gitlab-domains' => ['gitlab.com'],
            'use-github-api' => true,
            'notify-on-install' => true,
            'process-timeout' => 300,
            'platform' => [],
            'htaccess-protect' => true,
            'disable-tls' => false,
            'secure-http' => true,
            'lock' => true,
            'preferred-install' => 'dist',
            'archive-format' => 'tar',
            'archive-dir' => '.',
            'store-auths' => 'prompt',
            'allow-plugins' => [],
        ];

        public function __construct(bool $useEnvironment = true, ?string $baseDir = null)
        {
            $this->baseDir = $baseDir ?? getcwd();
            $this->config = self::$defaultConfig;

            // Set cache dir based on environment
            if ($useEnvironment) {
                $home = getenv('COMPOSER_HOME') ?: (
                    (getenv('HOME') ?: getenv('USERPROFILE')) . '/.composer'
                );
                $this->config['cache-dir'] = $home . '/cache';
            }
        }

        public function setBaseDir(?string $baseDir): void
        {
            $this->baseDir = $baseDir ?? getcwd();
        }

        public function setConfigSource(ConfigSourceInterface $source): void
        {
            $this->configSource = $source;
        }

        public function getConfigSource(): ?ConfigSourceInterface
        {
            return $this->configSource;
        }

        public function setAuthConfigSource(ConfigSourceInterface $source): void
        {
            $this->authConfigSource = $source;
        }

        public function getAuthConfigSource(): ?ConfigSourceInterface
        {
            return $this->authConfigSource;
        }

        public function merge(array $config, string $source = self::SOURCE_UNKNOWN): void
        {
            if (isset($config['config'])) {
                foreach ($config['config'] as $key => $value) {
                    $this->config[$key] = $value;
                }
            }
            if (isset($config['repositories'])) {
                $this->repositories = array_merge($this->repositories, $config['repositories']);
            }
        }

        public function getRepositories(): array
        {
            return $this->repositories;
        }

        /**
         * @return mixed
         */
        public function get(string $key, int $flags = 0)
        {
            $value = $this->config[$key] ?? null;

            if ($value === null) {
                return null;
            }

            // Handle path resolution
            if (is_string($value)) {
                $value = $this->resolveValue($value, $flags);
            }

            return $value;
        }

        private function resolveValue(string $value, int $flags): string
        {
            // Replace variables like {$vendor-dir}
            $value = preg_replace_callback('/\{\$(.+?)\}/', function ($match) use ($flags) {
                return (string) $this->get($match[1], $flags);
            }, $value);

            // Make paths absolute unless RELATIVE_PATHS flag is set
            if (!($flags & self::RELATIVE_PATHS) && $value !== '' && $value[0] !== '/' && !preg_match('/^[a-z]:/i', $value)) {
                $value = $this->baseDir . '/' . $value;
            }

            return $value;
        }

        public function all(int $flags = 0): array
        {
            $result = [];
            foreach (array_keys($this->config) as $key) {
                $result[$key] = $this->get($key, $flags);
            }
            return $result;
        }

        public function raw(): array
        {
            return $this->config;
        }

        public function has(string $key): bool
        {
            return array_key_exists($key, $this->config);
        }

        public static function disableProcessTimeout(): void
        {
            set_time_limit(0);
        }
    }

    /**
     * Loop/Process utilities stub
     */
    class Loop
    {
        public function wait(array $promises): void {}
    }
}

namespace Composer\Package {

    interface PackageInterface
    {
        public function getName(): string;
        public function getPrettyName(): string;
        public function getVersion(): string;
        public function getPrettyVersion(): string;
        public function getType(): string;
    }

    interface RootPackageInterface extends PackageInterface
    {
        public function getMinimumStability(): string;
        public function getStabilityFlags(): array;
        public function getReferences(): array;
        public function getAliases(): array;
    }

    class RootPackage implements RootPackageInterface
    {
        private string $name;
        private string $version;
        private string $type;
        private string $minimumStability;
        private array $extra = [];

        public function __construct(string $name = 'root/package', string $version = '1.0.0')
        {
            $this->name = $name;
            $this->version = $version;
            $this->type = 'project';
            $this->minimumStability = 'stable';
        }

        public function getName(): string { return $this->name; }
        public function getPrettyName(): string { return $this->name; }
        public function getVersion(): string { return $this->version; }
        public function getPrettyVersion(): string { return $this->version; }
        public function getType(): string { return $this->type; }
        public function getMinimumStability(): string { return $this->minimumStability; }
        public function getStabilityFlags(): array { return []; }
        public function getReferences(): array { return []; }
        public function getAliases(): array { return []; }
        public function getExtra(): array { return $this->extra; }
        public function setExtra(array $extra): void { $this->extra = $extra; }
    }
}

namespace Composer\Repository {

    interface RepositoryInterface {}

    class RepositoryManager
    {
        private array $repositories = [];

        public function getRepositories(): array
        {
            return $this->repositories;
        }

        public function addRepository(RepositoryInterface $repository): void
        {
            $this->repositories[] = $repository;
        }

        public function getLocalRepository(): ?RepositoryInterface
        {
            return null;
        }
    }
}

namespace Composer\Installer {

    class InstallationManager
    {
        public function getInstallPath($package): string
        {
            return 'vendor/' . $package->getName();
        }
    }
}

namespace Composer\Downloader {

    class DownloadManager {}
}

namespace Composer\Package\Archiver {

    class ArchiveManager {}
}

namespace Composer\Plugin {

    class PluginManager
    {
        public function getPlugins(): array
        {
            return [];
        }
    }
}

namespace Composer\Autoload {

    class AutoloadGenerator
    {
        public function dump($config, $localRepo, $package, $installationManager, $mainPackageDir, $scan = false, $suffix = null): int
        {
            return 0;
        }
    }
}

namespace Composer\Package\Locker {

    class Locker
    {
        public function isLocked(): bool { return false; }
        public function isFresh(): bool { return true; }
    }
}

namespace Composer\EventDispatcher {

    /**
     * Base Event class
     */
    class Event
    {
        protected string $name;
        protected array $args;
        protected array $flags;
        private bool $propagationStopped = false;

        public function __construct(string $name, array $args = [], array $flags = [])
        {
            $this->name = $name;
            $this->args = $args;
            $this->flags = $flags;
        }

        public function getName(): string
        {
            return $this->name;
        }

        public function getArguments(): array
        {
            return $this->args;
        }

        public function getFlags(): array
        {
            return $this->flags;
        }

        public function isPropagationStopped(): bool
        {
            return $this->propagationStopped;
        }

        public function stopPropagation(): void
        {
            $this->propagationStopped = true;
        }
    }

    class EventDispatcher
    {
        public function dispatch(string $eventName, Event $event = null): int
        {
            return 0;
        }

        public function dispatchScript(string $eventName, bool $devMode = false, array $additionalArgs = [], array $flags = []): int
        {
            return 0;
        }
    }
}

namespace Composer\Script {

    use Composer\Composer;
    use Composer\IO\IOInterface;
    use Composer\EventDispatcher\Event as BaseEvent;

    /**
     * Script Event - passed to Composer script callbacks
     */
    class Event extends BaseEvent
    {
        private Composer $composer;
        private IOInterface $io;
        private bool $devMode;
        private ?BaseEvent $originatingEvent = null;

        public function __construct(
            string $name,
            Composer $composer,
            IOInterface $io,
            bool $devMode = false,
            array $args = [],
            array $flags = []
        ) {
            parent::__construct($name, $args, $flags);
            $this->composer = $composer;
            $this->io = $io;
            $this->devMode = $devMode;
        }

        public function getComposer(): Composer
        {
            return $this->composer;
        }

        public function getIO(): IOInterface
        {
            return $this->io;
        }

        public function isDevMode(): bool
        {
            return $this->devMode;
        }

        public function getOriginatingEvent(): ?BaseEvent
        {
            return $this->originatingEvent;
        }

        public function setOriginatingEvent(BaseEvent $event): self
        {
            $this->originatingEvent = $this->calculateOriginatingEvent($event);
            return $this;
        }

        private function calculateOriginatingEvent(BaseEvent $event): BaseEvent
        {
            if ($event instanceof Event && $event->getOriginatingEvent() !== null) {
                return $this->calculateOriginatingEvent($event->getOriginatingEvent());
            }
            return $event;
        }
    }

    /**
     * Script event names
     */
    class ScriptEvents
    {
        public const PRE_INSTALL_CMD = 'pre-install-cmd';
        public const POST_INSTALL_CMD = 'post-install-cmd';
        public const PRE_UPDATE_CMD = 'pre-update-cmd';
        public const POST_UPDATE_CMD = 'post-update-cmd';
        public const PRE_STATUS_CMD = 'pre-status-cmd';
        public const POST_STATUS_CMD = 'post-status-cmd';
        public const PRE_ARCHIVE_CMD = 'pre-archive-cmd';
        public const POST_ARCHIVE_CMD = 'post-archive-cmd';
        public const PRE_AUTOLOAD_DUMP = 'pre-autoload-dump';
        public const POST_AUTOLOAD_DUMP = 'post-autoload-dump';
        public const POST_ROOT_PACKAGE_INSTALL = 'post-root-package-install';
        public const POST_CREATE_PROJECT_CMD = 'post-create-project-cmd';
        public const PRE_OPERATIONS_EXEC = 'pre-operations-exec';
    }
}

namespace Composer\Installer {

    use Composer\Composer;
    use Composer\IO\IOInterface;
    use Composer\EventDispatcher\Event as BaseEvent;
    use Composer\Package\PackageInterface;

    /**
     * Package Event - for package-specific operations
     */
    class PackageEvent extends BaseEvent
    {
        private Composer $composer;
        private IOInterface $io;
        private bool $devMode;
        private $operation;
        private array $installedRepo;

        public function __construct(
            string $eventName,
            Composer $composer,
            IOInterface $io,
            bool $devMode,
            $operation,
            array $installedRepo = []
        ) {
            parent::__construct($eventName);
            $this->composer = $composer;
            $this->io = $io;
            $this->devMode = $devMode;
            $this->operation = $operation;
            $this->installedRepo = $installedRepo;
        }

        public function getComposer(): Composer { return $this->composer; }
        public function getIO(): IOInterface { return $this->io; }
        public function isDevMode(): bool { return $this->devMode; }
        public function getOperation() { return $this->operation; }
        public function getInstalledRepo(): array { return $this->installedRepo; }
    }

    class InstallerEvent extends BaseEvent
    {
        private Composer $composer;
        private IOInterface $io;
        private bool $devMode;

        public function __construct(string $eventName, Composer $composer, IOInterface $io, bool $devMode)
        {
            parent::__construct($eventName);
            $this->composer = $composer;
            $this->io = $io;
            $this->devMode = $devMode;
        }

        public function getComposer(): Composer { return $this->composer; }
        public function getIO(): IOInterface { return $this->io; }
        public function isDevMode(): bool { return $this->devMode; }
    }

    class InstallerEvents
    {
        public const PRE_OPERATIONS_EXEC = 'pre-operations-exec';
        public const POST_DEPENDENCIES_SOLVING = 'post-dependencies-solving';
    }
}

namespace Composer {

    use Composer\IO\IOInterface;
    use Composer\Package\RootPackageInterface;
    use Composer\Package\RootPackage;
    use Composer\Repository\RepositoryManager;
    use Composer\Installer\InstallationManager;
    use Composer\Downloader\DownloadManager;
    use Composer\Package\Archiver\ArchiveManager;
    use Composer\Plugin\PluginManager;
    use Composer\Autoload\AutoloadGenerator;
    use Composer\Package\Locker\Locker;
    use Composer\EventDispatcher\EventDispatcher;

    /**
     * Main Composer class - complete implementation
     */
    class Composer
    {
        public const VERSION = '2.7.0-libretto';
        public const BRANCH_ALIAS_VERSION = '';
        public const RELEASE_DATE = '';
        public const SOURCE_VERSION = '';
        public const RUNTIME_API_VERSION = '2.2.2';

        private Config $config;
        private RootPackageInterface $package;
        private ?Locker $locker = null;
        private ?RepositoryManager $repositoryManager = null;
        private ?InstallationManager $installationManager = null;
        private ?DownloadManager $downloadManager = null;
        private ?ArchiveManager $archiveManager = null;
        private ?PluginManager $pluginManager = null;
        private ?AutoloadGenerator $autoloadGenerator = null;
        private ?EventDispatcher $eventDispatcher = null;
        private ?Loop $loop = null;
        private bool $global = false;

        public function __construct()
        {
            $this->config = new Config();
            $this->package = new RootPackage();
        }

        public static function getVersion(): string
        {
            return self::VERSION;
        }

        // Config
        public function setConfig(Config $config): void { $this->config = $config; }
        public function getConfig(): Config { return $this->config; }

        // Package
        public function setPackage(RootPackageInterface $package): void { $this->package = $package; }
        public function getPackage(): RootPackageInterface { return $this->package; }

        // Locker
        public function setLocker(Locker $locker): void { $this->locker = $locker; }
        public function getLocker(): ?Locker { return $this->locker; }

        // Repository Manager
        public function setRepositoryManager(RepositoryManager $manager): void { $this->repositoryManager = $manager; }
        public function getRepositoryManager(): ?RepositoryManager
        {
            return $this->repositoryManager ?? ($this->repositoryManager = new RepositoryManager());
        }

        // Installation Manager
        public function setInstallationManager(InstallationManager $manager): void { $this->installationManager = $manager; }
        public function getInstallationManager(): ?InstallationManager
        {
            return $this->installationManager ?? ($this->installationManager = new InstallationManager());
        }

        // Download Manager
        public function setDownloadManager(DownloadManager $manager): void { $this->downloadManager = $manager; }
        public function getDownloadManager(): ?DownloadManager
        {
            return $this->downloadManager ?? ($this->downloadManager = new DownloadManager());
        }

        // Archive Manager
        public function setArchiveManager(ArchiveManager $manager): void { $this->archiveManager = $manager; }
        public function getArchiveManager(): ?ArchiveManager
        {
            return $this->archiveManager ?? ($this->archiveManager = new ArchiveManager());
        }

        // Plugin Manager
        public function setPluginManager(PluginManager $manager): void { $this->pluginManager = $manager; }
        public function getPluginManager(): ?PluginManager
        {
            return $this->pluginManager ?? ($this->pluginManager = new PluginManager());
        }

        // Autoload Generator
        public function setAutoloadGenerator(AutoloadGenerator $generator): void { $this->autoloadGenerator = $generator; }
        public function getAutoloadGenerator(): ?AutoloadGenerator
        {
            return $this->autoloadGenerator ?? ($this->autoloadGenerator = new AutoloadGenerator());
        }

        // Event Dispatcher
        public function setEventDispatcher(EventDispatcher $dispatcher): void { $this->eventDispatcher = $dispatcher; }
        public function getEventDispatcher(): ?EventDispatcher
        {
            return $this->eventDispatcher ?? ($this->eventDispatcher = new EventDispatcher());
        }

        // Loop
        public function setLoop(Loop $loop): void { $this->loop = $loop; }
        public function getLoop(): ?Loop
        {
            return $this->loop ?? ($this->loop = new Loop());
        }

        // Global mode
        public function isGlobal(): bool { return $this->global; }
        public function setGlobal(): void { $this->global = true; }
    }
}

namespace {
    // Only define these if we're being loaded standalone (not when real Composer exists)
    // The check happens in the loader script that includes this file
}
