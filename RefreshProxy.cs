// RefreshProxy.cs — FreeView WinInet 프록시 설정 즉시 동기화 모듈
//
// 윈도우 레지스트리의 프록시/PAC 설정 변경을 운영체제 전역(커널 캐시 및 모든
// 인터넷 세션)에 즉시 반영하기 위해 wininet.dll의 InternetSetOption API를 호출합니다.
//
// 빌드 방법 (Developer Command Prompt):
//   csc /target:exe /out:refresh-proxy.exe RefreshProxy.cs
//
// 동작:
//   INTERNET_OPTION_SETTINGS_CHANGED (39): 레지스트리 변경 알림
//   INTERNET_OPTION_REFRESH (37): 프록시/PAC 캐시 강제 갱신
//
// PAC AutoConfigURL 및 수동 ProxyServer 변경 모두에 동일하게 동작합니다.

using System;
using System.Runtime.InteropServices;

class RefreshProxy
{
    [DllImport("wininet.dll", SetLastError = true)]
    private static extern bool InternetSetOption(
        IntPtr hInternet,
        int dwOption,
        IntPtr lpBuffer,
        int dwBufferLength
    );

    private const int INTERNET_OPTION_SETTINGS_CHANGED = 39;
    private const int INTERNET_OPTION_REFRESH = 37;

    static int Main()
    {
        try
        {
            // 레지스트리 변경 사항을 시스템에 알림
            InternetSetOption(IntPtr.Zero, INTERNET_OPTION_SETTINGS_CHANGED, IntPtr.Zero, 0);
            // 프록시/PAC 캐시를 강제 갱신하여 모든 브라우저에 즉시 적용
            InternetSetOption(IntPtr.Zero, INTERNET_OPTION_REFRESH, IntPtr.Zero, 0);
            return 0;
        }
        catch (Exception)
        {
            return 1;
        }
    }
}
