# RTFS Agent Communication Validation Script
# Validates compatibility of proposed agent communication features with existing RTFS codebase

param(
    [string]$RTFSRoot = ".",
    [switch]$Detailed = $false,
    [string]$OutputFile = ""
)

Write-Host "🔍 RTFS Agent Communication Feature Validation" -ForegroundColor Cyan
Write-Host "=============================================" -ForegroundColor Cyan

# Initialize results
$ValidationResults = @{
    "CompatibilityChecks" = @()
    "ConflictingFeatures" = @()
    "RequiredChanges" = @()
    "NewDependencies" = @()
    "Summary" = @{}
}

function Test-FileExists {
    param([string]$Path, [string]$Description)
    if (Test-Path $Path) {
        Write-Host "✅ $Description found: $Path" -ForegroundColor Green
        return $true
    } else {
        Write-Host "❌ $Description missing: $Path" -ForegroundColor Red
        return $false
    }
}

function Find-ConflictingCode {
    param([string]$SearchPath, [string]$Pattern, [string]$Description)
    
    $conflicts = @()
    if (Test-Path $SearchPath) {
        $files = Get-ChildItem -Path $SearchPath -Recurse -Include "*.rs", "*.md", "*.yaml", "*.yml" -ErrorAction SilentlyContinue
        foreach ($file in $files) {
            $content = Get-Content $file.FullName -Raw -ErrorAction SilentlyContinue
            if ($content -match $Pattern) {
                $conflicts += @{
                    "File" = $file.FullName
                    "Description" = $Description
                    "Pattern" = $Pattern
                }
            }
        }
    }
    return $conflicts
}

Write-Host "`n📁 Checking RTFS Project Structure..." -ForegroundColor Yellow

# Check core RTFS components
$coreChecks = @(
    @{ Path = "$RTFSRoot/rtfs_compiler"; Description = "RTFS Compiler" },
    @{ Path = "$RTFSRoot/specs"; Description = "RTFS Specifications" },
    @{ Path = "$RTFSRoot/examples"; Description = "RTFS Examples" },
    @{ Path = "$RTFSRoot/README.md"; Description = "Project README" }
)

foreach ($check in $coreChecks) {
    $result = Test-FileExists -Path $check.Path -Description $check.Description
    $ValidationResults.CompatibilityChecks += @{
        "Component" = $check.Description
        "Status" = if ($result) { "Found" } else { "Missing" }
        "Path" = $check.Path
    }
}

Write-Host "`n🔍 Analyzing Existing Communication Patterns..." -ForegroundColor Yellow

# Check for existing communication-related code
$communicationPatterns = @(
    @{ Pattern = "json.*rpc|JSON.*RPC"; Description = "Existing JSON-RPC usage" },
    @{ Pattern = "agent.*discovery|discovery.*agent"; Description = "Agent discovery references" },
    @{ Pattern = "websocket|WebSocket"; Description = "WebSocket usage" },
    @{ Pattern = "registry|Registry"; Description = "Registry patterns" },
    @{ Pattern = "delegate|delegation"; Description = "Task delegation" }
)

foreach ($pattern in $communicationPatterns) {
    $conflicts = Find-ConflictingCode -SearchPath "$RTFSRoot/rtfs_compiler/src" -Pattern $pattern.Pattern -Description $pattern.Description
    if ($conflicts.Count -gt 0) {
        Write-Host "⚠️  Found existing $($pattern.Description): $($conflicts.Count) instances" -ForegroundColor Yellow
        $ValidationResults.ConflictingFeatures += $conflicts
    } else {
        Write-Host "✅ No conflicts for $($pattern.Description)" -ForegroundColor Green
    }
}

Write-Host "`n📦 Checking Required Dependencies..." -ForegroundColor Yellow

# Check Rust dependencies that would be needed
$requiredDeps = @(
    "reqwest",
    "tokio-tungstenite", 
    "serde_json",
    "uuid",
    "chrono",
    "async-trait"
)

$cargoToml = "$RTFSRoot/rtfs_compiler/Cargo.toml"
if (Test-Path $cargoToml) {
    $cargoContent = Get-Content $cargoToml -Raw
    foreach ($dep in $requiredDeps) {
        if ($cargoContent -match "^$dep\s*=") {
            Write-Host "✅ Dependency already present: $dep" -ForegroundColor Green
        } else {
            Write-Host "➕ New dependency needed: $dep" -ForegroundColor Cyan
            $ValidationResults.NewDependencies += $dep
        }
    }
} else {
    Write-Host "❌ Cargo.toml not found - cannot check dependencies" -ForegroundColor Red
}

Write-Host "`n🏗️  Analyzing Required Code Changes..." -ForegroundColor Yellow

# Check for areas that would need modification
$modificationAreas = @(
    @{ Path = "$RTFSRoot/rtfs_compiler/src/main.rs"; Description = "Main compiler entry point" },
    @{ Path = "$RTFSRoot/rtfs_compiler/src/parser"; Description = "RTFS parser module" },
    @{ Path = "$RTFSRoot/rtfs_compiler/src/executor"; Description = "Task executor module" },
    @{ Path = "$RTFSRoot/specs/rtfs_specification.md"; Description = "Core specification" }
)

foreach ($area in $modificationAreas) {
    if (Test-Path $area.Path) {
        $ValidationResults.RequiredChanges += @{
            "Area" = $area.Description
            "Path" = $area.Path
            "Type" = "Extension"
            "Impact" = "Low-Medium"
        }
        Write-Host "🔧 Will need extension: $($area.Description)" -ForegroundColor Cyan
    } else {
        Write-Host "❓ Area not found: $($area.Description)" -ForegroundColor Yellow
    }
}

Write-Host "`n📊 Validation Summary" -ForegroundColor Yellow
Write-Host "===================" -ForegroundColor Yellow

$totalChecks = $ValidationResults.CompatibilityChecks.Count
$foundComponents = ($ValidationResults.CompatibilityChecks | Where-Object { $_.Status -eq "Found" }).Count
$conflictCount = $ValidationResults.ConflictingFeatures.Count
$newDepCount = $ValidationResults.NewDependencies.Count
$changeCount = $ValidationResults.RequiredChanges.Count

$ValidationResults.Summary = @{
    "ComponentsFound" = "$foundComponents/$totalChecks"
    "ConflictingFeatures" = $conflictCount
    "NewDependencies" = $newDepCount
    "RequiredChanges" = $changeCount
    "OverallCompatibility" = if ($foundComponents -ge ($totalChecks * 0.75) -and $conflictCount -eq 0) { "High" } elseif ($foundComponents -ge ($totalChecks * 0.5)) { "Medium" } else { "Low" }
}

Write-Host "📈 Components Found: $($ValidationResults.Summary.ComponentsFound)" -ForegroundColor $(if ($foundComponents -eq $totalChecks) { "Green" } else { "Yellow" })
Write-Host "⚠️  Conflicting Features: $($ValidationResults.Summary.ConflictingFeatures)" -ForegroundColor $(if ($conflictCount -eq 0) { "Green" } else { "Red" })
Write-Host "➕ New Dependencies: $($ValidationResults.Summary.NewDependencies)" -ForegroundColor Cyan
Write-Host "🔧 Required Changes: $($ValidationResults.Summary.RequiredChanges)" -ForegroundColor Cyan
Write-Host "🎯 Overall Compatibility: $($ValidationResults.Summary.OverallCompatibility)" -ForegroundColor $(
    switch ($ValidationResults.Summary.OverallCompatibility) {
        "High" { "Green" }
        "Medium" { "Yellow" }
        "Low" { "Red" }
        default { "Gray" }
    }
)

Write-Host "`n💡 Recommendations:" -ForegroundColor Yellow

if ($ValidationResults.Summary.OverallCompatibility -eq "High") {
    Write-Host "✅ Project structure is well-suited for agent communication features" -ForegroundColor Green
    Write-Host "✅ Minimal conflicts detected - implementation should be straightforward" -ForegroundColor Green
    Write-Host "📋 Consider implementing Phase 1 features first (Agent Discovery Protocol)" -ForegroundColor Cyan
}
elseif ($ValidationResults.Summary.OverallCompatibility -eq "Medium") {
    Write-Host "⚠️  Some adjustments needed but overall compatibility is good" -ForegroundColor Yellow
    Write-Host "🔍 Review conflicting features and plan integration carefully" -ForegroundColor Yellow
    Write-Host "📋 Start with core communication layer before advanced features" -ForegroundColor Cyan
}
else {
    Write-Host "❌ Significant challenges detected - thorough planning required" -ForegroundColor Red
    Write-Host "🔍 Address missing components before implementing agent communication" -ForegroundColor Red
    Write-Host "📋 Consider staged implementation approach" -ForegroundColor Cyan
}

# Output detailed results if requested
if ($Detailed) {
    Write-Host "`n📝 Detailed Results:" -ForegroundColor Yellow
    Write-Host "===================" -ForegroundColor Yellow
    
    if ($ValidationResults.ConflictingFeatures.Count -gt 0) {
        Write-Host "`n⚠️  Conflicting Features Details:" -ForegroundColor Red
        foreach ($conflict in $ValidationResults.ConflictingFeatures) {
            Write-Host "   File: $($conflict.File)" -ForegroundColor Gray
            Write-Host "   Issue: $($conflict.Description)" -ForegroundColor Gray
            Write-Host "   Pattern: $($conflict.Pattern)" -ForegroundColor Gray
            Write-Host ""
        }
    }
    
    if ($ValidationResults.NewDependencies.Count -gt 0) {
        Write-Host "`n➕ New Dependencies to Add:" -ForegroundColor Cyan
        foreach ($dep in $ValidationResults.NewDependencies) {
            Write-Host "   - $dep" -ForegroundColor Cyan
        }
    }
}

# Save results to file if requested
if ($OutputFile) {
    $jsonResults = $ValidationResults | ConvertTo-Json -Depth 10
    $jsonResults | Out-File -FilePath $OutputFile -Encoding UTF8
    Write-Host "`n💾 Results saved to: $OutputFile" -ForegroundColor Green
}

Write-Host "`n🏁 Validation Complete!" -ForegroundColor Green
