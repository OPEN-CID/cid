param(
    [Parameter(Mandatory=$true)]
    [string]$Prompt
)


$models = @(
    "openrouter/qwen/qwen3-coder:free",
    "openrouter/deepseek/deepseek-v4-flash:free",
    "openrouter/google/gemini-3.6-flash",
    "openrouter/qwen/qwen3-coder"
)


foreach($model in $models)
{

    Write-Host ""
    Write-Host "======================================" -ForegroundColor Cyan
    Write-Host "Trying model: $model" -ForegroundColor Yellow
    Write-Host "======================================"


    try
    {

        opencode run `
        --model $model `
        --max-tokens 4000 `
        "$Prompt"


        if($LASTEXITCODE -eq 0)
        {
            Write-Host ""
            Write-Host "SUCCESS: $model" -ForegroundColor Green
            exit 0
        }

    }
    catch
    {
        Write-Host "Failed: $model" -ForegroundColor Red
    }


    Write-Host "Switching to next model..." -ForegroundColor Yellow

}


Write-Host ""
Write-Host "All models failed" -ForegroundColor Red

exit 1
