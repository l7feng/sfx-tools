# Recreate S1.0 synthetic fixtures; never overwrite existing files.
$ErrorActionPreference = 'Stop'
$ffmpeg = (Get-Command ffmpeg -ErrorAction Stop).Source
$ffprobe = (Get-Command ffprobe -ErrorAction Stop).Source
$sampleRoot = Join-Path $PSScriptRoot 'build/s1-samples'
$cases = @(
    @{ Name='pcm-44100.wav'; Codec='pcm_s16le'; Encoder='pcm_s16le'; Rate=44100; Duration='0.5'; Min=0.49; Max=0.51; Format='wav' },
    @{ Name='pcm-48000.aiff'; Codec='pcm_s16be'; Encoder='pcm_s16be'; Rate=48000; Duration='0.5'; Min=0.49; Max=0.51; Format='aiff' },
    @{ Name='lossless-48000.flac'; Codec='flac'; Encoder='flac'; Rate=48000; Duration='0.5'; Min=0.49; Max=0.51; Format='flac' },
    @{ Name='lossy-44100.mp3'; Codec='mp3'; Encoder='libmp3lame'; Rate=44100; Duration='0.5'; Min=0.45; Max=0.65; Format='mp3' },
    @{ Name='long-44100.wav'; Codec='pcm_s16le'; Encoder='pcm_s16le'; Rate=44100; Duration='5'; Min=4.99; Max=5.01; Format='wav' }
)

function Assert-Fixture($Case, [string]$Path) {
    if (!(Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Not a regular file: $Path" }
    $json = & $ffprobe -v error -show_entries 'stream=codec_type,codec_name,sample_rate,channels:format=duration,format_name' -of json $Path
    if ($LASTEXITCODE -ne 0) { throw "ffprobe failed: $Path" }
    $data = ($json -join [Environment]::NewLine) | ConvertFrom-Json
    $streams = @($data.streams)
    if ($streams.Count -ne 1 -or $streams[0].codec_type -ne 'audio') { throw "Expected exactly one audio stream: $Path" }
    $audio = $streams[0]
    if ($audio.codec_name -ne $Case.Codec -or [int]$audio.sample_rate -ne $Case.Rate -or [int]$audio.channels -ne 1) {
        throw "Existing fixture has unexpected codec/rate/channels; not overwritten: $Path"
    }
    if (@($data.format.format_name -split ',') -notcontains $Case.Format) { throw "Unexpected container: $Path" }
    $seconds = [double]::Parse([string]$data.format.duration, [Globalization.CultureInfo]::InvariantCulture)
    if ($seconds -lt $Case.Min -or $seconds -gt $Case.Max) { throw "Unexpected fixture duration $seconds; not overwritten: $Path" }
    return $seconds
}

# Validate every existing object before creating any missing sample.
foreach ($case in $cases) {
    $path = Join-Path $sampleRoot $case.Name
    if (Test-Path -LiteralPath $path) { $null = Assert-Fixture $case $path }
}
if (!(Test-Path -LiteralPath $sampleRoot)) { New-Item -ItemType Directory -Path $sampleRoot -Force | Out-Null }
foreach ($case in $cases) {
    $path = Join-Path $sampleRoot $case.Name
    $action = 'verified-existing'
    if (!(Test-Path -LiteralPath $path)) {
        & $ffmpeg -nostdin -n -hide_banner -loglevel error -f lavfi -i "sine=frequency=440:duration=$($case.Duration)" -ar $case.Rate -ac 1 -c:a $case.Encoder $path
        if ($LASTEXITCODE -ne 0) { throw "Fixture generation failed; any partial file is retained for inspection: $path" }
        $action = 'generated'
    }
    $seconds = Assert-Fixture $case $path
    [pscustomobject]@{
        File=$case.Name; Action=$action; Codec=$case.Codec; SampleRate=$case.Rate
        Channels=1; Seconds=$seconds; SHA256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    }
}
