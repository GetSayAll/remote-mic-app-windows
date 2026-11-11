import { remoteModelLabel, type ConnectionSnapshot, type RemoteModel } from "./bridge";
import { t } from "./i18n";

const REMOTE_MODEL_KEY = "sayall.last-recognized-remote-model";

export function loadRemoteModelHint(): RemoteModel {
  try {
    const model = localStorage.getItem(REMOTE_MODEL_KEY);
    if (model === "chromecast" || model === "rc001" || model === "rc003") return model;
  } catch {
    // The model hint only affects the offline view; live identification still works.
  }
  return "unknown";
}

export function storeRemoteModelHint(model: Exclude<RemoteModel, "unknown">): void {
  try {
    localStorage.setItem(REMOTE_MODEL_KEY, model);
  } catch {
    // Some preview environments disable storage.
  }
}

export function recognizedRemoteModel(connection?: ConnectionSnapshot | null): RemoteModel {
  if (connection && connection.remoteModel !== "unknown") return connection.remoteModel;
  return connection?.remoteName?.trim().toLowerCase() === "chromecast remote"
    ? "chromecast"
    : "unknown";
}

export function remoteArtwork(model: RemoteModel): { src: string; alt: string } {
  return model === "chromecast"
    ? { src: "/chromecast-remote.svg", alt: remoteModelLabel(model) }
    : { src: "/RC003-remote-photo@2x.webp", alt: t("小米蓝牙语音遥控器示意图") };
}
