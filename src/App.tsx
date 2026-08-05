import React, { useState, useEffect, useRef } from 'react';
import { ShieldCheck, Sun, Moon, Power, Terminal, X, Minus } from 'lucide-react';

const electronAPI = (window as any).electronAPI;
const isElectron = typeof window !== 'undefined' && !!electronAPI;

export default function App() {
  const [isServiceRunning, setIsServiceRunning] = useState(false);
  const [isTransitioning, setIsTransitioning] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [theme, setTheme] = useState<'light' | 'dark'>('light');
  const logsEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // 테마 설정 초기화
    const savedTheme = localStorage.getItem('theme') as 'light' | 'dark' | null;
    const initialTheme = savedTheme || 'light';
    setTheme(initialTheme);
    document.documentElement.setAttribute('data-theme', initialTheme);

    if (isElectron) {
      // 초기 상태 조회
      electronAPI.getInitialState().then((state: { isServiceRunning: boolean; logs: string[] }) => {
        setIsServiceRunning(state.isServiceRunning);
        setLogs(state.logs);
      });

      // 로그 업데이트 수신
      electronAPI.onLogUpdate((log: string) => {
        setLogs((prev) => [...prev, log]);
      });

      // 서비스 상태 업데이트 수신
      electronAPI.onStatusUpdate((status: boolean) => {
        setIsServiceRunning(status);
      });
    } else {
      // 웹 브라우저 환경 테스트용 시뮬레이션
      setLogs([
        "[19:40:00] FreeView Web Mock Mode",
        "[19:40:01] ✓ System standby (Clean start)."
      ]);
    }
  }, []);

  // 로그 창 자동 스크롤
  useEffect(() => {
    if (logsEndRef.current) {
      logsEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [logs]);

  const toggleTheme = () => {
    const nextTheme = theme === 'light' ? 'dark' : 'light';
    setTheme(nextTheme);
    localStorage.setItem('theme', nextTheme);
    document.documentElement.setAttribute('data-theme', nextTheme);
  };

  const handleToggleService = async () => {
    if (isTransitioning) return;
    setIsTransitioning(true);
    if (isElectron) {
      try {
        await electronAPI.toggleService();
      } catch (e) {
        // ignore
      } finally {
        setIsTransitioning(false);
      }
    } else {
      // 시뮬레이션 동작
      setTimeout(() => {
        const nextState = !isServiceRunning;
        setIsServiceRunning(nextState);
        if (nextState) {
          setLogs((prev) => [
            ...prev,
            `[${new Date().toLocaleTimeString('ko-KR', { hour12: false })}] System initializing...`,
            `[${new Date().toLocaleTimeString('ko-KR', { hour12: false })}] 로컬 프록시 서버 시작 (127.0.0.1:8080)`,
            `[${new Date().toLocaleTimeString('ko-KR', { hour12: false })}] DNS 라우팅 재설정 (Cloudflare)`,
            `[${new Date().toLocaleTimeString('ko-KR', { hour12: false })}] SNI 터널링 활성화`,
            `[${new Date().toLocaleTimeString('ko-KR', { hour12: false })}] ✓ Service is fully operational.`
          ]);
        } else {
          setLogs((prev) => [
            ...prev,
            `[${new Date().toLocaleTimeString('ko-KR', { hour12: false })}] Stopping services...`,
            `[${new Date().toLocaleTimeString('ko-KR', { hour12: false })}] 터널링 해제`,
            `[${new Date().toLocaleTimeString('ko-KR', { hour12: false })}] DNS 설정 복원 (DHCP)`,
            `[${new Date().toLocaleTimeString('ko-KR', { hour12: false })}] ✓ System standby.`
          ]);
        }
        setIsTransitioning(false);
      }, 800);
    }
  };

  return (
    <div className="pdf-app pdf-grid-bg" style={{ paddingTop: '32px', height: '100vh', display: 'flex', flexDirection: 'column' }}>
      
      {/* 1. PDF-DS 스타일 커스텀 타이틀바 */}
      <div className="custom-titlebar" style={{
        position: 'fixed',
        top: 0,
        left: 0,
        right: 0,
        height: '32px',
        backgroundColor: 'var(--color-bg-primary)',
        borderBottom: '1px solid var(--color-border-default)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        zIndex: 9999,
        paddingLeft: '10px',
        userSelect: 'none',
        WebkitUserSelect: 'none',
        WebkitAppRegion: 'drag' as any
      }}>
        <div className="custom-titlebar-left" style={{ display: 'flex', alignItems: 'center', gap: '8px', WebkitAppRegion: 'no-drag' as any }}>
          <ShieldCheck size={16} className="pdf-text-red" />
          <button
            type="button"
            className="titlebar-settings-btn"
            onClick={toggleTheme}
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              width: '24px',
              height: '24px',
              borderRadius: '4px',
              border: '1px solid var(--color-border-default)',
              backgroundColor: 'var(--color-bg-secondary)',
              color: 'var(--color-text-primary)',
              cursor: 'pointer',
              transition: 'all 0.2s'
            }}
            title="테마 변경"
          >
            {theme === 'light' ? <Moon size={12} /> : <Sun size={12} />}
          </button>
        </div>
        
        <div className="custom-titlebar-center" style={{
          fontSize: '13px',
          fontWeight: 700,
          color: 'var(--color-text-primary)',
          position: 'absolute',
          left: '50%',
          transform: 'translateX(-50%)',
          pointerEvents: 'none'
        }}>
          FreeView
        </div>
        
        <div className="custom-titlebar-right" style={{ display: 'flex', height: '100%', alignItems: 'center', WebkitAppRegion: 'no-drag' as any }}>
          {isElectron && (
            <>
              <button
                type="button"
                className="titlebar-win-btn"
                onClick={() => electronAPI.minimize()}
                style={{
                  width: '46px',
                  height: '100%',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  color: 'var(--color-text-primary)',
                  border: 'none',
                  background: 'transparent',
                  cursor: 'pointer',
                  transition: 'background-color 0.2s'
                }}
                title="최소화"
              >
                <Minus size={12} />
              </button>
              <button
                type="button"
                className="titlebar-win-btn titlebar-close-btn"
                onClick={() => electronAPI.close()}
                style={{
                  width: '46px',
                  height: '100%',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  color: 'var(--color-text-primary)',
                  border: 'none',
                  background: 'transparent',
                  cursor: 'pointer',
                  transition: 'background-color 0.2s'
                }}
                title="닫기"
              >
                <X size={12} />
              </button>
            </>
          )}
        </div>
      </div>

      {/* 2. 메인 뷰포트 영역 */}
      <div className="pdf-content-relative" style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '24px 20px',
        boxSizing: 'border-box',
        overflow: 'hidden'
      }}>
        
        {/* 상단 타이틀 로고 영역 */}
        <div className="pdf-text-center" style={{ marginTop: '16px' }}>
          <h1 className="pdf-text-heading-32" style={{ fontWeight: 800, color: 'var(--color-text-primary)' }}>FreeView</h1>
          <p className="pdf-text-copy-14 pdf-text-muted" style={{ marginTop: '4px' }}>Version 2.0.0</p>
        </div>

        {/* 중앙 전원 제어 영역 */}
        <div className="pdf-flex-col pdf-items-center pdf-justify-center" style={{ flex: 1, gap: '12px' }}>
          <button
            type="button"
            onClick={handleToggleService}
            className={`pdf-btn-primary ${!isServiceRunning ? 'off-state' : ''} ${isTransitioning ? 'anim-pulse' : ''}`}
            style={{
              width: '140px',
              height: '140px',
              borderRadius: '50%',
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: '8px',
              border: '1px solid var(--color-border-default)',
              backgroundColor: isTransitioning
                ? 'var(--color-bg-secondary)'
                : (isServiceRunning ? 'var(--color-functional-red)' : 'var(--color-bg-primary)'),
              color: isTransitioning
                ? 'var(--color-text-primary)'
                : (isServiceRunning ? '#ffffff' : 'var(--color-text-secondary)'),
              boxShadow: isTransitioning
                ? 'none'
                : (isServiceRunning ? 'var(--shadow-functional-glow)' : 'var(--shadow-hardware-bevel)'),
              transition: 'all 0.3s cubic-bezier(0.4, 0, 0.2, 1)',
              cursor: isTransitioning ? 'not-allowed' : 'pointer'
            }}
            disabled={isTransitioning}
          >
            <Power 
              size={36} 
              className={isTransitioning ? 'anim-spin' : ''} 
              style={{ 
                transform: isServiceRunning ? 'scale(1.1)' : 'scale(1)',
                transition: 'transform 0.3s'
              }} 
            />
            <span style={{ fontSize: '12px', fontWeight: 700, letterSpacing: '0.5px' }}>
              {isTransitioning 
                ? (isServiceRunning ? 'STOPPING...' : 'STARTING...') 
                : (isServiceRunning ? 'ACTIVE' : 'OFF')}
            </span>
          </button>
          
          <div className="pdf-flex-row pdf-items-center pdf-gap-050" style={{ marginTop: '8px' }}>
            <div className="pdf-indicator-dot" style={{
              backgroundColor: isServiceRunning ? 'var(--color-functional-red)' : 'var(--color-border-hover)',
              boxShadow: isServiceRunning ? '0 0 8px var(--color-functional-red)' : 'none',
              transition: 'all 0.3s'
            }}></div>
            <span className="pdf-text-copy-13-mono pdf-text-muted">
              {isServiceRunning ? '터널링 가동 중' : '대기 상태'}
            </span>
          </div>
        </div>

        {/* 하단 시스템 로그 패널 영역 */}
        <div className="pdf-w-full" style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
          <div className="pdf-flex-row pdf-items-center pdf-gap-050" style={{ paddingLeft: '4px' }}>
            <Terminal size={14} className="pdf-text-muted" />
            <span className="pdf-text-label-16" style={{ fontSize: '11px', color: 'var(--color-text-secondary)', fontWeight: 700 }}>
              SYSTEM STATUS
            </span>
          </div>
          
          <div className="pdf-panel" style={{
            height: '160px',
            overflowY: 'auto',
            padding: '12px',
            backgroundColor: 'var(--color-bg-secondary)',
            border: '1px solid var(--color-border-default)',
            borderRadius: '8px',
            margin: 0,
            display: 'flex',
            flexDirection: 'column',
            gap: '6px'
          }}>
            {logs.map((log, index) => {
              const isError = log.includes('Error') || log.includes('실패') || log.includes('오류');
              const isSuccess = log.includes('✓');
              let textColor = 'var(--color-text-primary)';
              if (isError) textColor = 'var(--color-functional-red)';
              else if (isSuccess) textColor = 'var(--color-functional-red)'; // PDF-DS Accent color
              else if (log.startsWith('[')) {
                // Time prefix color styling
                const endBracketIdx = log.indexOf(']');
                if (endBracketIdx !== -1) {
                  return (
                    <div key={index} className="pdf-text-copy-13-mono" style={{ color: 'var(--color-text-primary)', wordBreak: 'break-all' }}>
                      <span style={{ color: 'var(--color-text-secondary)' }}>
                        {log.substring(0, endBracketIdx + 1)}
                      </span>
                      {log.substring(endBracketIdx + 1)}
                    </div>
                  );
                }
              }
              
              return (
                <div key={index} className="pdf-text-copy-13-mono" style={{ color: textColor, wordBreak: 'break-all' }}>
                  {log}
                </div>
              );
            })}
            <div ref={logsEndRef} />
          </div>
        </div>

        {/* 메타데이터 푸터 */}
        <div className="pdf-w-full pdf-text-center" style={{ marginTop: '12px', fontSize: '10px', color: 'var(--color-text-secondary)', fontFamily: 'var(--font-mono)' }}>
          <div>쿼터파이 © 2024 - 2026</div>
        </div>

      </div>

      {/* 타이틀바 및 애니메이션 스타일 주입 */}
      <style>{`
        .titlebar-win-btn:hover {
          background-color: var(--color-bg-secondary);
        }
        .titlebar-close-btn:hover {
          background-color: var(--color-functional-red) !important;
          color: #ffffff !important;
        }
        .off-state:hover {
          border-color: var(--color-border-hover) !important;
          color: var(--color-text-primary) !important;
          background-color: var(--color-bg-secondary) !important;
        }
        @keyframes pulse-glow {
          0% { box-shadow: 0 0 4px var(--color-border-default); }
          50% { box-shadow: 0 0 16px var(--color-functional-red); }
          100% { box-shadow: 0 0 4px var(--color-border-default); }
        }
        @keyframes spin-slow {
          from { transform: rotate(0deg); }
          to { transform: rotate(360deg); }
        }
        .anim-pulse {
          animation: pulse-glow 1.5s infinite ease-in-out;
        }
        .anim-spin {
          animation: spin-slow 1.2s infinite linear;
          transform-origin: center;
        }
      `}</style>
    </div>
  );
}
