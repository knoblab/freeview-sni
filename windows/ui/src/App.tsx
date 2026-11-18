import React, { useEffect, useRef, useState } from 'react';
import { appBridge } from './bridge';

export default function App() {
  const [isServiceRunning, setIsServiceRunning] = useState(false);
  const [isTransitioning, setIsTransitioning] = useState(false);
  const [version, setVersion] = useState('3.3.2');
  const [updateLabel, setUpdateLabel] = useState('업데이트 확인');
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);

  const updateTimerRef = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlistenStatus: (() => void) | null = null;

    appBridge.getInitialState().then((state) => {
      if (cancelled) return;
      setIsServiceRunning(state.isServiceRunning);
      if (state.version) {
        setVersion(state.version);
      }
    });

    appBridge
      .onStatusUpdate((status) => {
        if (cancelled) return;
        setIsServiceRunning(status.isRunning);
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
    try {
      const status = await appBridge.toggleService();
      setIsServiceRunning(status.isRunning);
    } finally {
      setIsTransitioning(false);
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
    <div className="fv-window">
      <div className="fv-topbar" onMouseDown={handleTopBarMouseDown}>
        <span className="fv-title">FreeView {version}</span>
      </div>

      <button
        type="button"
        className={`fv-toggle-btn ${isServiceRunning ? 'is-on' : ''}`}
        onClick={handleToggleService}
        disabled={isTransitioning}
      >
        {isServiceRunning ? '끄기' : '켜기'}
      </button>

      <button
        type="button"
        className="fv-secondary-btn fv-log-btn"
        onClick={() => appBridge.openLogFile()}
      >
        로그 보기
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
  );
}
