$Project="C:\Projects\cid"

cd $Project


while($true)
{

Write-Host ""
Write-Host "======================================"
Write-Host "CID Autonomous Cycle $(Get-Date)"
Write-Host "======================================"


try
{

$prompt = Get-Content ".\cid-autonomous-prompt.md" -Raw


$prompt | opencode run --auto


}
catch
{

Write-Host "CID agent failed. Restarting..." -ForegroundColor Yellow

}


Write-Host "Waiting before next cycle..."

Start-Sleep -Seconds 30


}
