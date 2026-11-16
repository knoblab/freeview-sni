import React, { useState, useEffect, useRef } from 'react';
import { ShieldCheck, Sun, Moon, Power, Terminal, X, Minus } from 'lucide-react';
import { appBridge } from './bridge';

interface LogEntry {
  id: number;
  text: string;
}

const MAX_UI_LOGS = 200;

export default function App() {
  const [isServiceRunning, setIsServiceRunning] = useState(false);
  const [port, setPort] = useState(8080);
  const [isTransitioning, setIsTransitioning] = useState(false);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [theme, setTheme] = useState<'light' | 'dark'>('light');
  const [version, setVersion] = useState('3.2.2');
  const [isPageVisible, setIsPageVisible] = useState<boolean>(() => !document.hidden);

  const logsContainerRef = useRef<HTMLDivElement>(null);
  const nextLogIdRef = useRef(0);
  const isNearBottomRef = useRef(true);

  useEffect(() => {
    const savedTheme = localStorage.getItem('theme') as 'light' | 'dark' | null;
    const initialTheme = savedTheme || 'light';
    setTheme(initialTheme);
    document.documentElement.setAttribute('data-theme', initialTheme);

    let cancelled = false;
    let unlistenLog: (() => void) | null = null;
    let unlistenStatus: (() => void) | null = null;

    appBridge.getInitialState().then((state) => {
      if (cancelled) return;
      setIsServiceRunning(state.isServiceRunning);
      if (state.port) setPort(state.port);
      setLogs(
        state.logs.slice(-MAX_UI_LOGS).map((text) => ({
          id: ++nextLogIdRef.current,
          text,
        })),
      );
      if (state.version) {
        setVersion(state.version);
      }
    });

    appBridge
      .onLogUpdate((log) => {
        if (cancelled) return;
        const id = ++nextLogIdRef.current;
        setLogs((prev) => {
          const next = [...prev, { id, text: log }];
          return next.length > MAX_UI_LOGS ? next.slice(next.length - MAX_UI_LOGS) : next;
        });
      })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
        } else {
          unlistenLog = unlisten;
        }
      });

    appBridge
      .onStatusUpdate((status) => {
        if (cancelled) return;
        setIsServiceRunning(status.isRunning);
        if (status.port) setPort(status.port);
      })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
        } else {
          unlistenStatus = unlisten;
        }
      });

    const handleVisibilityChange = () => {
      setIsPageVisible(!document.hidden);
    };
    document.addEventListener('visibilitychange', handleVisibilityChange);

    return () => {
      cancelled = true;
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      if (unlistenLog) unlistenLog();
      if (unlistenStatus) unlistenStatus();
    };
  }, []);

  // Scroll log panel using rAF without smooth-scroll CPU churn
  useEffect(() => {
    if (!isNearBottomRef.current) return;
    const el = logsContainerRef.current;
    if (!el) return;
    const frameId = requestAnimationFrame(() => {
      el.scrollTop = el.scrollHeight;
    });
    return () => cancelAnimationFrame(frameId);
  }, [logs]);

  const handleLogsScroll = () => {
    const el = logsContainerRef.current;
    if (!el) return;
    isNearBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  const toggleTheme = () => {
    const nextTheme = theme === 'light' ? 'dark' : 'light';
    setTheme(nextTheme);
    localStorage.setItem('theme', nextTheme);
    document.documentElement.setAttribute('data-theme', nextTheme);
  };

  const handleTitleBarMouseDown = async (e: React.MouseEvent) => {
    if (e.button === 0) {
      if ((e.target as HTMLElement).closest('button')) {
        return;
      }
      await appBridge.startDragging();
    }
  };

  const handleToggleService = async () => {
    if (isTransitioning) return;
    setIsTransitioning(true);
    try {
      const status = await appBridge.toggleService();
      setIsServiceRunning(status.isRunning);
      if (status.port) setPort(status.port);
    } catch (e) {
      console.error(e);
    } finally {
      setIsTransitioning(false);
    }
  };

  const animPausedClass = isPageVisible ? '' : 'anim-paused';

  return (
    <div
      className="pdf-app pdf-grid-bg"
      style={{ paddingTop: '32px', height: '100vh', display: 'flex', flexDirection: 'column' }}
    >
      {/* 1. Custom Titlebar */}
      <div
        className="custom-titlebar"
        onMouseDown={handleTitleBarMouseDown}
        style={{
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
          cursor: 'default',
        }}
      >
        <div className="custom-titlebar-left" style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
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
              transition: 'background-color 0.2s, border-color 0.2s',
            }}
            title="Theme Toggle"
          >
            {theme === 'light' ? <Moon size={12} /> : <Sun size={12} />}
          </button>
        </div>

        <div
          className="custom-titlebar-center"
          style={{
            fontSize: '13px',
            fontWeight: 700,
            color: 'var(--color-text-primary)',
            position: 'absolute',
            left: '50%',
            transform: 'translateX(-50%)',
            pointerEvents: 'none',
          }}
        >
          FreeView
        </div>

        <div className="custom-titlebar-right" style={{ display: 'flex', height: '100%', alignItems: 'center' }}>
          <button
            type="button"
            className="titlebar-win-btn"
            onClick={() => appBridge.minimize()}
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
              transition: 'background-color 0.2s',
            }}
            title="Minimize"
          >
            <Minus size={12} />
          </button>
          <button
            type="button"
            className="titlebar-win-btn titlebar-close-btn"
            onClick={() => appBridge.close()}
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
              transition: 'background-color 0.2s',
            }}
            title="Close"
          >
            <X size={12} />
          </button>
        </div>
      </div>

      {/* 2. Main Viewport */}
      <div
        className="pdf-content-relative"
        style={{
          flex: 1,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '24px 20px',
          boxSizing: 'border-box',
          overflow: 'hidden',
        }}
      >
        {/* Title & Version */}
        <div
          className="pdf-text-center"
          onMouseDown={handleTitleBarMouseDown}
          style={{ marginTop: '16px', cursor: 'default', userSelect: 'none' }}
        >
          <h1 className="pdf-text-heading-32" style={{ fontWeight: 800, color: 'var(--color-text-primary)' }}>
            FreeView
          </h1>
          <p className="pdf-text-copy-14 pdf-text-muted" style={{ marginTop: '4px' }}>
            Version {version}
          </p>
        </div>

        {/* Center Power Button */}
        <div className="pdf-flex-col pdf-items-center pdf-justify-center" style={{ flex: 1, gap: '12px' }}>
          <button
            type="button"
            onClick={handleToggleService}
            disabled={isTransitioning}
            className={`pdf-btn-primary ${isServiceRunning ? `anim-pulse ${animPausedClass}` : 'off-state'}`}
            style={{
              position: 'relative',
              width: '120px',
              height: '120px',
              borderRadius: '50%',
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: '6px',
              border: isServiceRunning ? 'none' : '1px solid var(--color-border-default)',
              backgroundColor: isServiceRunning ? 'var(--color-functional-red)' : 'var(--color-bg-primary)',
              color: isServiceRunning ? '#ffffff' : 'var(--color-text-secondary)',
              cursor: isTransitioning ? 'not-allowed' : 'pointer',
              boxShadow: isServiceRunning ? 'var(--shadow-functional-glow)' : 'var(--shadow-hardware-bevel)',
              transition: 'background-color 0.25s ease, color 0.25s ease, border-color 0.25s ease',
            }}
          >
            <Power
              size={36}
              className={isTransitioning ? `anim-spin ${animPausedClass}` : ''}
              style={{
                transform: isServiceRunning ? 'scale(1.1)' : 'scale(1)',
                transition: 'transform 0.25s ease',
              }}
            />
            <span style={{ fontSize: '12px', fontWeight: 700, letterSpacing: '0.5px' }}>
              {isTransitioning ? (isServiceRunning ? 'STOPPING...' : 'STARTING...') : isServiceRunning ? 'ACTIVE' : 'OFF'}
            </span>
          </button>

          <div className="pdf-flex-row pdf-items-center pdf-gap-050" style={{ marginTop: '8px' }}>
            <div
              className="pdf-indicator-dot"
              style={{
                backgroundColor: isServiceRunning ? 'var(--color-functional-red)' : 'var(--color-border-hover)',
                boxShadow: isServiceRunning ? '0 0 8px var(--color-functional-red)' : 'none',
                transition: 'background-color 0.25s ease',
              }}
            />
            <span className="pdf-text-copy-13-mono pdf-text-muted">
              {isServiceRunning ? `보호 가동 중 (127.0.0.1:${port})` : '대기 상태 (보호 꺼짐)'}
            </span>
          </div>
        </div>

        {/* Logs Panel */}
        <div className="pdf-w-full" style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
          <div className="pdf-flex-row pdf-items-center pdf-gap-050" style={{ paddingLeft: '4px' }}>
            <Terminal size={14} className="pdf-text-muted" />
            <span
              className="pdf-text-label-16"
              style={{ fontSize: '11px', color: 'var(--color-text-secondary)', fontWeight: 700 }}
            >
              SYSTEM STATUS
            </span>
          </div>

          <div
            ref={logsContainerRef}
            onScroll={handleLogsScroll}
            className="pdf-panel"
            style={{
              height: '160px',
              overflowY: 'auto',
              padding: '12px',
              backgroundColor: 'var(--color-bg-secondary)',
              border: '1px solid var(--color-border-default)',
              borderRadius: '8px',
              margin: 0,
              display: 'flex',
              flexDirection: 'column',
              gap: '6px',
            }}
          >
            {logs.map((entry) => {
              const log = entry.text;
              const isError = log.includes('Error') || log.includes('실패') || log.includes('오류');
              const isSuccess = log.includes('✓');
              let textColor = 'var(--color-text-primary)';
              if (isError) textColor = 'var(--color-functional-red)';
              else if (isSuccess) textColor = 'var(--color-functional-red)';
              else if (log.startsWith('[')) {
                const endBracketIdx = log.indexOf(']');
                if (endBracketIdx !== -1) {
                  return (
                    <div
                      key={entry.id}
                      className="pdf-text-copy-13-mono"
                      style={{ color: 'var(--color-text-primary)', wordBreak: 'break-all' }}
                    >
                      <span style={{ color: 'var(--color-text-secondary)' }}>{log.substring(0, endBracketIdx + 1)}</span>
                      {log.substring(endBracketIdx + 1)}
                    </div>
                  );
                }
              }

              return (
                <div key={entry.id} className="pdf-text-copy-13-mono" style={{ color: textColor, wordBreak: 'break-all' }}>
                  {log}
                </div>
              );
            })}
          </div>
        </div>

        {/* Footer */}
        <div
          className="pdf-w-full pdf-text-center"
          style={{ marginTop: '12px', fontSize: '10px', color: 'var(--color-text-secondary)', fontFamily: 'var(--font-mono)' }}
        >
          <div>쿼터파이 © 2024 - 2026</div>
        </div>
      </div>

      {/* Component animations (GPU-composited opacity/transform only, paused when hidden) */}
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
        @keyframes pulse-ring {
          0%, 100% { opacity: 0.12; transform: scale(0.98); }
          50% { opacity: 0.75; transform: scale(1.04); }
        }
        @keyframes spin-slow {
          from { transform: rotate(0deg); }
          to { transform: rotate(360deg); }
        }
        .anim-pulse::after {
          content: "";
          position: absolute;
          inset: -6px;
          border-radius: 50%;
          border: 2px solid var(--color-functional-red);
          box-shadow: 0 0 14px var(--color-functional-red);
          pointer-events: none;
          will-change: opacity, transform;
          animation: pulse-ring 2s infinite ease-in-out;
        }
        .anim-spin {
          animation: spin-slow 1.2s infinite linear;
          transform-origin: center;
        }
        .anim-paused,
        .anim-paused::after {
          animation-play-state: paused !important;
        }
      `}</style>
    </div>
  );
}
