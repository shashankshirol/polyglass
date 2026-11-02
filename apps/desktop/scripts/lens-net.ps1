# Windows-only PRIV-01 check (M1-VER-1): records every socket the running app and its child
# processes (the WebView2 browser, renderer, GPU and network processes) hold, polling the IP
# Helper tables for -Seconds. Start the app first; drive it meanwhile (lens-trace.ps1 moves the
# lens over text, so every move runs capture + OCR + translation). Prints each TCP connection
# to a non-loopback address and each UDP endpoint not bound to loopback, with the process that
# held it, and exits 1 if there was any.
#   Start-Process target\release\polyglass.exe -ArgumentList '--trace-timings=uncached'
#   powershell -File apps\desktop\scripts\lens-net.ps1 -Seconds 120
# -Id watches another process tree instead (a control run: a process that does connect).
# Limits: polling misses a connection that opens and closes between two polls (a few ms apart);
# name lookups run in the DNS Client service, not in the app, so they are invisible here; TIME_WAIT
# entries belong to no process. A packet capture (needs admin) or switching the network off (the
# other half of PRIV-01) closes those gaps.
param([int]$Seconds = 120, [int]$IntervalMs = 5, [string]$Name = "polyglass", [int]$Id = 0)

Add-Type @"
using System; using System.Collections.Generic; using System.Net; using System.Runtime.InteropServices;
public static class IpTables {
  [DllImport("iphlpapi.dll")] static extern uint GetExtendedTcpTable(IntPtr t, ref int size, bool sort, int af, int cls, int r);
  [DllImport("iphlpapi.dll")] static extern uint GetExtendedUdpTable(IntPtr t, ref int size, bool sort, int af, int cls, int r);
  static int Port(IntPtr p, int at) { int v = Marshal.ReadInt32(p, at); return ((v & 0xFF) << 8) | ((v >> 8) & 0xFF); }
  static string Addr(IntPtr p, int at, int len) { var b = new byte[len]; Marshal.Copy(IntPtr.Add(p, at), b, 0, len); return new IPAddress(b).ToString(); }
  delegate uint Get(IntPtr t, ref int size);
  static IntPtr Fetch(Get get) {
    int size = 0; get(IntPtr.Zero, ref size);
    while (true) {
      IntPtr buf = Marshal.AllocHGlobal(size);
      if (get(buf, ref size) == 0) return buf;
      Marshal.FreeHGlobal(buf);
    }
  }
  // Rows: "tcp|remote address|remote port|pid|state" and "udp|local address|local port|pid".
  public static List<string> Rows() {
    var rows = new List<string>();
    foreach (int af in new[] { 2, 23 }) {
      bool v6 = af == 23;
      IntPtr t = Fetch((IntPtr b, ref int s) => GetExtendedTcpTable(b, ref s, false, af, 5, 0));  // TCP_TABLE_OWNER_PID_ALL
      int n = Marshal.ReadInt32(t), rowSize = v6 ? 56 : 24;
      for (int i = 0; i < n; i++) {
        int r = 4 + i * rowSize;
        if (v6) rows.Add("tcp|" + Addr(t, r + 24, 16) + "|" + Port(t, r + 44) + "|" + Marshal.ReadInt32(t, r + 52) + "|" + Marshal.ReadInt32(t, r + 48));
        else rows.Add("tcp|" + Addr(t, r + 12, 4) + "|" + Port(t, r + 16) + "|" + Marshal.ReadInt32(t, r + 20) + "|" + Marshal.ReadInt32(t, r));
      }
      Marshal.FreeHGlobal(t);
      IntPtr u = Fetch((IntPtr b, ref int s) => GetExtendedUdpTable(b, ref s, false, af, 1, 0));  // UDP_TABLE_OWNER_PID
      n = Marshal.ReadInt32(u); rowSize = v6 ? 28 : 12;
      for (int i = 0; i < n; i++) {
        int r = 4 + i * rowSize;
        if (v6) rows.Add("udp|" + Addr(u, r, 16) + "|" + Port(u, r + 20) + "|" + Marshal.ReadInt32(u, r + 24));
        else rows.Add("udp|" + Addr(u, r, 4) + "|" + Port(u, r + 4) + "|" + Marshal.ReadInt32(u, r + 8));
      }
      Marshal.FreeHGlobal(u);
    }
    return rows;
  }
}
"@

$root = if ($Id) { Get-Process -Id $Id -ErrorAction SilentlyContinue }
        else { Get-Process -Name $Name -ErrorAction SilentlyContinue | Select-Object -First 1 }
if (-not $root) { "no $Name process"; exit 2 }

function Tree([int]$rootId) {
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name
  $ids = @{ $rootId = $root.ProcessName }
  do {
    $n = $ids.Count
    foreach ($p in $all) {
      if ($ids.ContainsKey([int]$p.ParentProcessId) -and -not $ids.ContainsKey([int]$p.ProcessId)) {
        $ids[[int]$p.ProcessId] = $p.Name
      }
    }
  } while ($ids.Count -gt $n)
  $ids
}

# Loopback, or unspecified (a listening socket or a UDP socket not bound to an address).
$loopback = @('127.0.0.1', '::1')
$unspecified = @('0.0.0.0', '::')
$tcpStates = @{ 1 = 'CLOSED'; 2 = 'LISTEN'; 3 = 'SYN_SENT'; 4 = 'SYN_RCVD'; 5 = 'ESTABLISHED';
  6 = 'FIN_WAIT1'; 7 = 'FIN_WAIT2'; 8 = 'CLOSE_WAIT'; 9 = 'CLOSING'; 10 = 'LAST_ACK';
  11 = 'TIME_WAIT'; 12 = 'DELETE_TCB' }

$seen = @{}
$polls = 0
$tree = Tree $root.Id
$treeAt = Get-Date
$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end -and -not $root.HasExited) {
  # WebView2 utility processes come and go; refresh the tree every 2 s.
  if (((Get-Date) - $treeAt).TotalSeconds -ge 2) { $tree = Tree $root.Id; $treeAt = Get-Date }
  foreach ($row in [IpTables]::Rows()) {
    $f = $row.Split('|')
    $owner = [int]$f[3]
    if (-not $tree.ContainsKey($owner) -or $loopback -contains $f[1]) { continue }
    if ($f[0] -eq 'tcp') {
      if ($unspecified -contains $f[1]) { continue }
      $what = "$($tcpStates[[int]$f[4]])"
    } else {
      $what = "local endpoint"
    }
    $key = "$($f[0]) $($f[1]):$($f[2]) $($tree[$owner])"
    if (-not $seen.ContainsKey($key)) { $seen[$key] = "$key  ($what, pid $owner, $(Get-Date -Format HH:mm:ss.fff))" }
  }
  $polls++
  Start-Sleep -Milliseconds $IntervalMs
}
$names = ($tree.Values | Group-Object | ForEach-Object { "$($_.Count)x $($_.Name)" }) -join ', '
"$polls polls over $Seconds s; process tree at the end: $names"
if ($seen.Count -eq 0) { "no network endpoints outside loopback"; exit 0 }
$seen.Values | Sort-Object
exit 1
