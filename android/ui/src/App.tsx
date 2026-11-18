import React, { useEffect, useRef, useState } from 'react';
import { appBridge, IS_ANDROID } from './bridge';

export default function App() {
  const [isServiceRunning, setIsServiceRunning] = useState(false);
  const [isTransitioning, setIsTransitioning] = useState(false);
  const [version, setVersion] = useState('3.3.2');
  const [desyncMode, setDesyncMode] = useState('TlsHeader5');
  const [delayMs, setDelayMs] = useState(2);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [showLogs, setShowLogs] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [updateLabel, setUpdateLabel] = useState('업데이트 확인');
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);

  const updateTimerRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlistenStatus: (() => void) | null = null;

    appBridge.getInitialState().then((state) => {
      if (cancelled) return;
      setIsServiceRunning(state.isServiceRunning);
      if (state.version) setVersion(state.version);
      if (state.desyncMode) setDesyncMode(state.desyncMode);
      if (typeof state.delayMs === 'number') setDelayMs(state.delayMs);
      if (state.logs) setLogs(state.logs);
    });

    appBridge
      .onStatusUpdate((status) => {
        if (cancelled) return;
        setIsServiceRunning(status.isRunning);
        setErrorMsg(status.error ?? null);
      })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
        } else {
          unlistenStatus = unlisten;
        }
      });

    return () => {
      cancelled = true;
      if (unlistenStatus) unlistenStatus();
      if (updateTimerRef.current !== null) {
        window.clearTimeout(updateTimerRef.current);
      }
    };
  }, []);

  const handleTopBarMouseDown = async (e: React.MouseEvent) => {
    if (e.button === 0) {
      await appBridge.startDragging();
    }
  };

  const handleToggleService = async () => {
    if (isTransitioning) return;
    setIsTransitioning(true);
    setErrorMsg(null);
    try {
      const status = await appBridge.toggleService(desyncMode, delayMs);
      setIsServiceRunning(status.isRunning);
      if (status.error) {
        setErrorMsg(status.error);
      }
      if (showLogs) {
        const latestLogs = await appBridge.getLogs();
        setLogs(latestLogs);
      }
    } finally {
      setIsTransitioning(false);
    }
  };

  const handleShowLogs = async () => {
    if (IS_ANDROID) {
      if (!showLogs) {
        const latestLogs = await appBridge.getLogs();
        setLogs(latestLogs);
      }
      setShowLogs((v) => !v);
    } else {
      await appBridge.openLogFile();
    }
  };

  const handleCheckUpdate = async () => {
    if (isCheckingUpdate) return;
    setIsCheckingUpdate(true);
    setUpdateLabel('확인 중...');
    if (updateTimerRef.current !== null) {
      window.clearTimeout(updateTimerRef.current);
      updateTimerRef.current = null;
    }

    try {
      const res = await appBridge.checkUpdate();
      if (res.status === 'up_to_date') {
        setUpdateLabel('최신 버전입니다');
      } else if (res.status === 'update_available') {
        setUpdateLabel(res.latestVersion ? `새 버전 ${res.latestVersion}` : '새 버전 열림');
      } else {
        setUpdateLabel('확인 실패');
      }
    } finally {
      setIsCheckingUpdate(false);
      updateTimerRef.current = window.setTimeout(() => {
        setUpdateLabel('업데이트 확인');
        updateTimerRef.current = null;
      }, 2500);
    }
  };

  return (
    <div className={`fv-window ${IS_ANDROID ? 'is-mobile' : ''}`}>
      <div className="fv-topbar" onMouseDown={handleTopBarMouseDown}>
        <span className="fv-title">FreeView {version}</span>
      </div>

      <button
        type="button"
        className={`fv-toggle-btn ${isServiceRunning ? 'is-on' : ''}`}
        onClick={handleToggleService}
        disabled={isTransitioning}
      >
        {isTransitioning ? '전환 중...' : isServiceRunning ? '끄기' : '켜기'}
      </button>

      {IS_ANDROID && (
        <div className="fv-mobile-status">
          {errorMsg
            ? `오류: ${errorMsg}`
            : isServiceRunning
              ? `VPN 보호 활성화 (${desyncMode}, ${delayMs}ms)`
              : '대기 중 — 켜기를 눌러 VPN 보호를 시작하세요'}
        </div>
      )}

      <div className="fv-actions">
        <button
          type="button"
          className="fv-secondary-btn fv-log-btn"
          onClick={handleShowLogs}
        >
          {IS_ANDROID && showLogs ? '로그 닫기' : '로그 보기'}
        </button>

        <button
          type="button"
          className="fv-secondary-btn fv-github-btn"
          onClick={() => appBridge.openGithub()}
        >
          깃허브
        </button>

        <button
          type="button"
          className="fv-secondary-btn fv-update-btn"
          onClick={handleCheckUpdate}
          disabled={isCheckingUpdate}
        >
          {updateLabel}
        </button>
      </div>

      {IS_ANDROID && showLogs && (
        <div className="fv-mobile-logs">
          {logs.length === 0 ? (
            <div className="fv-log-empty">기록된 로그가 없습니다.</div>
          ) : (
            logs.slice(-50).map((line, idx) => (
              <div key={idx} className="fv-log-line">
                {line}
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}
