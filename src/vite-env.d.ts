/// <reference types="vite/client" />

interface Window {
  __TAURI__?: {
    core?: {
      invoke: <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
    };
  };
  __DESK_PP__?: {
    setOpacity: (val: number) => void;
    positionLocked?: boolean;
    invoke?: <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
  };
}
