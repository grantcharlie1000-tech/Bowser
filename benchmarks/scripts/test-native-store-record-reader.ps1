# Deterministic Windows sharing contract, not a diagnosis of an earlier native commit failure.
[CmdletBinding()]
param([string]$Out="$env:RUNNER_TEMP/bowser-native-store/record-reader-contract.json")
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
if([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT){throw 'This record sharing contract requires Windows.'}
Import-Module (Join-Path $PSScriptRoot 'native-store-record-reader.psm1') -Force
# Bowser commits records with Rust std::fs::rename, not .NET File.Move. On Windows std first calls
# MoveFileExW(MOVEFILE_REPLACE_EXISTING); only on ERROR_ACCESS_DENIED does it retry through
# FileRenameInfoEx with REPLACE_IF_EXISTS|POSIX_SEMANTICS, and if that also fails it returns the
# original error (DIR_NOT_EMPTY excepted). File.Move stops at the MoveFileExW denial, so it cannot
# model whether a Delete-sharing reader lets Bowser's actual replacement through.
if(-not ('BowserStdRename' -as [type])){
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class BowserStdRename {
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool MoveFileExW(string existing, string replacement, uint flags);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern SafeFileHandle CreateFileW(string name, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool SetFileInformationByHandle(SafeFileHandle handle, int informationClass, byte[] information, uint size);
    static Exception Failure(int code) {
        string message = "std::fs::rename equivalent failed with Win32 error " + code + ".";
        if (code == 5) return new UnauthorizedAccessException(message);
        return new IOException(message, unchecked((int)0x80070000) | code);
    }
    public static void Rename(string source, string destination) {
        if (MoveFileExW(source, destination, 0x1)) return;
        int original = Marshal.GetLastWin32Error();
        if (original != 5) throw Failure(original);
        using (SafeFileHandle handle = CreateFileW(source, 0x00010000, 0x7, IntPtr.Zero, 3, 0x00200000 | 0x02000000, IntPtr.Zero)) {
            if (handle.IsInvalid) throw Failure(original);
            char[] name = destination.ToCharArray();
            // FILE_RENAME_INFO: Flags union (pointer aligned), RootDirectory, FileNameLength, FileName.
            int offset = IntPtr.Size + IntPtr.Size + 4;
            int length = name.Length * 2;
            byte[] information = new byte[offset + length + 2];
            BitConverter.GetBytes(0x1 | 0x2).CopyTo(information, 0);
            BitConverter.GetBytes(length).CopyTo(information, IntPtr.Size + IntPtr.Size);
            Buffer.BlockCopy(name, 0, information, offset, length);
            if (!SetFileInformationByHandle(handle, 22, information, (uint)information.Length)) {
                int code = Marshal.GetLastWin32Error();
                throw Failure(code == 145 ? 145 : original);
            }
        }
    }
}
'@
}
$root=Join-Path ([IO.Path]::GetTempPath()) ('bowser-record-reader-contract-'+[guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
$file=Join-Path $root 'extensions.txt';$temp=Join-Path $root 'next.txt'
$utf8=[Text.UTF8Encoding]::new($false,$true)
$old="ext-0000000000000001`t1`tSynthetic extension`t1.0`n"
$new="ext-0000000000000001`t0`tSynthetic extension`t1.0`n"
$report=[ordered]@{scope='Windows read-share contract on generated disposable files';status='failed';powershell_version=$PSVersionTable.PSVersion.ToString();old_share_blocks_replace=$false;control_error_code=$null;control_hresult=$null;control_exception_type=$null;control_error_kind=$null;released_control_allows_replace=$false;delete_share_allows_replace=$false;open_reader_retains_old_generation=$false;new_reader_observes_new_generation=$false;oversize_refused=$false;invalid_utf8_refused=$false}
try {
 [IO.File]::WriteAllText($file,$old,$utf8);[IO.File]::WriteAllText($temp,$new,$utf8)
 # This matches the documented Get-Content provider's ReadWrite sharing, not its full implementation.
 $control=[IO.FileStream]::new($file,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
 try {
  try {[BowserStdRename]::Rename($temp,$file);throw 'ReadWrite-only control unexpectedly allowed replacement.'}
  catch {
   $failure=$_.Exception.GetBaseException();$code=$failure.HResult -band 0xffff
   $report.control_error_code=$code;$report.control_hresult=$failure.HResult
   $typeName=$failure.GetType().FullName
   if(!$typeName -or $typeName.Length -gt 128){throw 'Control exception type was unavailable or over bound.'}
   $report.control_exception_type=$typeName
   $report.control_error_kind=if($failure -is [UnauthorizedAccessException]){'unauthorized-access'}elseif($failure -is [IO.IOException]){'io'}else{'other'}
   # MoveFileExW reports an undeletable destination as ERROR_ACCESS_DENIED, which std then returns.
   if(!(($code -eq 32 -and $failure -is [IO.IOException]) -or ($code -eq 5 -and $failure -is [UnauthorizedAccessException]))){throw}
   $report.old_share_blocks_replace=$true
  }
 } finally {$control.Dispose()}
 if((Read-BowserRecordSnapshot $file) -cne $old -or ![IO.File]::Exists($temp)){throw 'Failed replacement changed the control files.'}
 # The identical operation must succeed after releasing only the known blocking handle.
 [BowserStdRename]::Rename($temp,$file)
 if((Read-BowserRecordSnapshot $file) -cne $new -or [IO.File]::Exists($temp)){throw 'Released control did not permit the same replacement.'}
 $report.released_control_allows_replace=$true
 [IO.File]::WriteAllText($file,$old,$utf8);[IO.File]::WriteAllText($temp,$new,$utf8)
 $reader=Open-BowserRecordSnapshot $file
 try {
  [BowserStdRename]::Rename($temp,$file);$report.delete_share_allows_replace=$true
  $bytes=[byte[]]::new($utf8.GetByteCount($old)+1);$count=0
  do {$read=$reader.Read($bytes,$count,$bytes.Length-$count);$count+=$read} while($read -gt 0 -and $count -lt $bytes.Length)
  if($utf8.GetString($bytes,0,$count) -cne $old){throw 'Open reader did not retain the old atomic generation.'}
  $report.open_reader_retains_old_generation=$true
 } finally {$reader.Dispose()}
 if((Read-BowserRecordSnapshot $file) -cne $new){throw 'Fresh reader did not observe the new committed generation.'}
 $report.new_reader_observes_new_generation=$true
 [IO.File]::WriteAllBytes($file,[byte[]]::new(1048577))
 try {$null=Read-BowserRecordSnapshot $file;throw 'Oversized record observation was accepted.'}
 catch {if($_.Exception.Message -cne 'Native record observation exceeded its byte bound.'){throw};$report.oversize_refused=$true}
 [IO.File]::WriteAllBytes($file,[byte[]]@(0xc3,0x28))
 try {$null=Read-BowserRecordSnapshot $file;throw 'Invalid UTF-8 record observation was accepted.'}
 catch {if($_.Exception.GetBaseException() -isnot [Text.DecoderFallbackException]){throw};$report.invalid_utf8_refused=$true}
 $report.status='passed-generated-file-sharing-contract'
} finally {
 [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Out)))
 [IO.File]::WriteAllText($Out,($report|ConvertTo-Json -Depth 4),$utf8)
 [IO.Directory]::Delete($root,$true)
}
Write-Output 'RECORD_READER_CONTRACT readwrite_control_blocked=true released_control_replace=true delete_share_replace=true old_and_new_generations_checked=true bound_refused=true invalid_utf8_refused=true'
