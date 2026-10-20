# Read-only test observations must not deny Bowser's atomic record replacement.
# These helpers do not acquire authority, retry mutations or accept a changed record.
Set-StrictMode -Version Latest
function Open-BowserRecordSnapshot {
 param([Parameter(Mandatory)][string]$Path)
 return [IO.FileStream]::new($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
}
function Read-BowserRecordSnapshot {
 param([Parameter(Mandatory)][string]$Path)
 $limit=1048576
 $stream=Open-BowserRecordSnapshot $Path
 try {
  $bytes=[byte[]]::new($limit+1);$count=0
  do {
   $read=$stream.Read($bytes,$count,$bytes.Length-$count)
   $count+=$read
  } while($read -gt 0 -and $count -lt $bytes.Length)
  if($count -gt $limit){throw 'Native record observation exceeded its byte bound.'}
  return [Text.UTF8Encoding]::new($false,$true).GetString($bytes,0,$count)
 } finally { $stream.Dispose() }
}
Export-ModuleMember -Function Open-BowserRecordSnapshot,Read-BowserRecordSnapshot
