import { Channel, invoke } from "@tauri-apps/api/core";

import type { RootAccessStatus } from "./library-roots";

export type FsChangeKind =
  "create" | "modify" | "move" | "delete" | "rescan-required";

export type FsRescanReason =
  | "ambiguous-rename"
  | "batch-overflow"
  | "queue-overflow"
  | "channel-disconnected"
  | "out-of-scope"
  | "unknown-event"
  | "watcher-error";

export interface FsChange {
  kind: FsChangeKind;
  paths: string[];
  reason?: FsRescanReason;
}

export interface FsChangeBatch {
  root: string;
  changes: FsChange[];
  rawEventCount: number;
}

export type LibraryWatchEvent =
  | { event: "started"; data: { watchId: string; rootId: string } }
  | {
      event: "changes";
      data: { watchId: string; rootId: string; batch: FsChangeBatch };
    }
  | {
      event: "failed";
      data: {
        watchId: string;
        rootId: string;
        message: string;
        rootAccessStatus: RootAccessStatus | null;
      };
    }
  | { event: "stopped"; data: { watchId: string; rootId: string } };

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

interface WatchChannel {
  onmessage: (message: LibraryWatchEvent) => void;
}

type ChannelFactory = () => WatchChannel;

export function startLibraryWatch(
  rootId: string,
  receive: (event: LibraryWatchEvent) => void,
  call: Invoke = invoke,
  createChannel: ChannelFactory = () => new Channel<LibraryWatchEvent>(),
): Promise<string> {
  const onEvent = createChannel();
  onEvent.onmessage = receive;
  return call<string>("start_library_watch", { rootId, onEvent });
}

export function stopLibraryWatch(
  watchId: string,
  call: Invoke = invoke,
): Promise<boolean> {
  return call<boolean>("stop_library_watch", { watchId });
}
