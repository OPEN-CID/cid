$Project="C:\Projects\cid"

cd $Project


while($true)
{

Write-Host "Starting CID autonomous cycle..." -ForegroundColor Green


opencode run `
--auto `
"
Read:

CID-detailed-doc.md

CID Phase prompts.

Find current incomplete phase.

Continue implementation.

Rules:

1. Do not stop after explanation.
2. Write code.
3. Run tests.
4. Fix failures.
5. Update progress notes.
6. Continue until milestone complete.

"


Write-Host "Cycle finished. Restarting..."


Start-Sleep -Seconds 30

}
