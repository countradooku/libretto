#!/bin/bash

# Comprehensive benchmark: Libretto Turbo vs Composer
# Tests various Laravel dependency scenarios

LIBRETTO="../target/release/libretto"
RESULTS_FILE="benchmark_results.txt"

echo "=============================================="
echo "  Libretto Turbo vs Composer Benchmark"
echo "=============================================="
echo ""
echo "System: $(uname -s) $(uname -m)"
echo "Composer: $(composer --version 2>/dev/null | head -1)"
echo "Libretto: $($LIBRETTO --version 2>/dev/null || echo 'dev')"
echo "Date: $(date)"
echo ""

# Clear caches for fair comparison
echo "Clearing caches..."
composer clear-cache -q 2>/dev/null
rm -rf vendor composer.lock 2>/dev/null

run_benchmark() {
    local name="$1"
    local composer_json="$2"

    echo ""
    echo "=============================================="
    echo "  TEST: $name"
    echo "=============================================="

    # Write composer.json
    echo "$composer_json" > composer.json
    rm -f composer.lock

    echo ""
    echo "Dependencies:"
    cat composer.json | grep -A 100 '"require"' | head -20
    echo ""

    # Benchmark Composer (cold cache)
    echo "--- Composer (cold cache) ---"
    composer clear-cache -q 2>/dev/null
    time_start=$(date +%s.%N)
    composer update --dry-run --no-interaction 2>&1 | tail -5
    time_end=$(date +%s.%N)
    composer_cold=$(echo "$time_end - $time_start" | bc)
    echo "Time: ${composer_cold}s"

    # Benchmark Composer (warm cache)
    echo ""
    echo "--- Composer (warm cache) ---"
    time_start=$(date +%s.%N)
    composer update --dry-run --no-interaction 2>&1 | tail -5
    time_end=$(date +%s.%N)
    composer_warm=$(echo "$time_end - $time_start" | bc)
    echo "Time: ${composer_warm}s"

    # Benchmark Libretto Turbo
    echo ""
    echo "--- Libretto Turbo ---"
    time_start=$(date +%s.%N)
    $LIBRETTO install --dry-run --turbo 2>&1 | tail -10
    time_end=$(date +%s.%N)
    libretto_turbo=$(echo "$time_end - $time_start" | bc)
    echo "Time: ${libretto_turbo}s"

    # Benchmark Libretto Standard
    echo ""
    echo "--- Libretto Standard ---"
    time_start=$(date +%s.%N)
    timeout 60 $LIBRETTO install --dry-run 2>&1 | tail -5
    time_end=$(date +%s.%N)
    libretto_std=$(echo "$time_end - $time_start" | bc)
    echo "Time: ${libretto_std}s"

    # Calculate speedups
    echo ""
    echo "=== RESULTS: $name ==="
    echo "Composer (cold):     ${composer_cold}s"
    echo "Composer (warm):     ${composer_warm}s"
    echo "Libretto Turbo:      ${libretto_turbo}s"
    echo "Libretto Standard:   ${libretto_std}s"

    speedup_vs_cold=$(echo "scale=2; $composer_cold / $libretto_turbo" | bc)
    speedup_vs_warm=$(echo "scale=2; $composer_warm / $libretto_turbo" | bc)
    echo ""
    echo "Turbo vs Composer (cold): ${speedup_vs_cold}x faster"
    echo "Turbo vs Composer (warm): ${speedup_vs_warm}x faster"
}

# Test 1: Basic Laravel (current)
run_benchmark "Basic Laravel 11" '{
    "name": "laravel/laravel",
    "type": "project",
    "require": {
        "php": "^8.2",
        "laravel/framework": "^11.0",
        "laravel/tinker": "^2.9"
    },
    "require-dev": {
        "fakerphp/faker": "^1.23",
        "laravel/pail": "^1.0",
        "laravel/sail": "^1.26",
        "mockery/mockery": "^1.6",
        "nunomaduro/collision": "^8.0",
        "phpunit/phpunit": "^11.0"
    },
    "minimum-stability": "stable",
    "prefer-stable": true
}'

# Test 2: Laravel with common packages
run_benchmark "Laravel + Common Packages" '{
    "name": "laravel/laravel",
    "type": "project",
    "require": {
        "php": "^8.2",
        "laravel/framework": "^11.0",
        "laravel/tinker": "^2.9",
        "laravel/sanctum": "^4.0",
        "laravel/horizon": "^5.0",
        "spatie/laravel-permission": "^6.0",
        "spatie/laravel-medialibrary": "^11.0",
        "inertiajs/inertia-laravel": "^1.0"
    },
    "require-dev": {
        "fakerphp/faker": "^1.23",
        "laravel/sail": "^1.26",
        "phpunit/phpunit": "^11.0"
    },
    "minimum-stability": "stable",
    "prefer-stable": true
}'

# Test 3: API-heavy Laravel
run_benchmark "Laravel API Project" '{
    "name": "laravel/laravel",
    "type": "project",
    "require": {
        "php": "^8.2",
        "laravel/framework": "^11.0",
        "laravel/sanctum": "^4.0",
        "spatie/laravel-query-builder": "^6.0",
        "spatie/laravel-data": "^4.0",
        "league/fractal": "^0.20",
        "predis/predis": "^2.0"
    },
    "require-dev": {
        "fakerphp/faker": "^1.23",
        "phpunit/phpunit": "^11.0"
    },
    "minimum-stability": "stable",
    "prefer-stable": true
}'

# Test 4: Minimal (just framework)
run_benchmark "Minimal Laravel" '{
    "name": "laravel/laravel",
    "type": "project",
    "require": {
        "php": "^8.2",
        "laravel/framework": "^11.0"
    },
    "minimum-stability": "stable",
    "prefer-stable": true
}'

echo ""
echo "=============================================="
echo "  Benchmark Complete!"
echo "=============================================="
