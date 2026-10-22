import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type MediaKind = "image" | "video";

export interface Media {
  id: string;
  model_id: string;
  kind: MediaKind;
  file_name: string;
  ext: string;
  size: number;
  sha256: string;
  width: number | null;
  height: number | null;
  duration: number | null;
  taken_at: string | null;
  imported_at: number;
  has_thumb: boolean;
}

export interface ModelInfo {
  id: string;
  name: string;
  created: number;
  cover: string | null;
  images: number;
  videos: number;
  bytes: number;
}

export interface Settings {
  vault_path: string | null;
  auto_lock_minutes: number;
  content_protection: boolean;
  lock_on_windows_lock: boolean;
}

export interface AppInfo {
  settings: Settings;
  vault_found: boolean;
  hostname: string;
  ffmpeg: boolean;
}

export interface Status {
  unlocked: boolean;
  read_only: boolean;
  holder: { host: string; since: number; stale: boolean } | null;
  vault_path: string | null;
  busy: boolean;
}

export interface ImportProgress {
  model_id: string;
  total: number;
  done: number;
  imported: number;
  duplicates: number;
  current: string;
  errors: string[];
  unsupported: string[];
  finished: boolean;
  cancelled: boolean;
}

export interface ExportProgress {
  phase: "write" | "verify" | "done";
  done: number;
  total: number;
  finished: boolean;
  error: string | null;
  files: number;
}

export type ExportFormat = "zip" | "7z";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  status: () => invoke<Status>("status"),
  createVault: (parent: string, password: string) =>
    invoke<{ recovery_key: string; vault_path: string }>("create_vault", { parent, password }),
  unlock: (path: string | null, password: string) => invoke<Status>("unlock", { path, password }),
  unlockRecovery: (path: string | null, recoveryKey: string, newPassword: string) =>
    invoke<Status>("unlock_recovery", { path, recoveryKey, newPassword }),
  takeOverLock: () => invoke<Status>("take_over_lock"),
  lock: () => invoke<void>("lock_vault"),
  touch: () => invoke<void>("touch"),
  listModels: () => invoke<ModelInfo[]>("list_models"),
  createModel: (name: string) => invoke<ModelInfo>("create_model", { name }),
  renameModel: (id: string, name: string) => invoke<void>("rename_model", { id, name }),
  deleteModel: (id: string) => invoke<void>("delete_model", { id }),
  setCover: (modelId: string, mediaId: string) => invoke<void>("set_cover", { modelId, mediaId }),
  listMedia: (modelId: string) => invoke<Media[]>("list_media", { modelId }),
  deleteMedia: (ids: string[]) => invoke<void>("delete_media", { ids }),
  moveMedia: (ids: string[], targetModelId: string) => invoke<void>("move_media", { ids, targetModelId }),
  startImport: (modelId: string, paths: string[]) => invoke<void>("start_import", { modelId, paths }),
  startExport: (model_ids: string[], format: ExportFormat, password: string, dest: string) =>
    invoke<void>("start_export", { request: { model_ids, format, password, dest } }),
  cancelJob: () => invoke<void>("cancel_job"),
  changePassword: (oldPassword: string, newPassword: string) =>
    invoke<void>("change_password", { oldPassword, newPassword }),
  saveSettings: (auto_lock_minutes: number, content_protection: boolean, lock_on_windows_lock: boolean) =>
    invoke<Settings>("save_settings", { update: { auto_lock_minutes, content_protection, lock_on_windows_lock } }),
  forgetVault: () => invoke<void>("forget_vault"),
  generatePassword: () => invoke<string>("generate_password"),
};

export const thumbUrl = (id: string) => convertFileSrc(`t-${id}`, "vault");
export const mediaUrl = (id: string) => convertFileSrc(`m-${id}`, "vault");

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toLocaleString("de-DE", { maximumFractionDigits: v < 10 ? 1 : 0 })} ${units[i]}`;
}

export function formatDuration(s: number | null): string {
  if (s == null) return "";
  const m = Math.floor(s / 60);
  const sec = Math.round(s % 60);
  return `${m}:${sec.toString().padStart(2, "0")}`;
}

export function formatDate(secs: number): string {
  return new Date(secs * 1000).toLocaleString("de-DE", { dateStyle: "medium", timeStyle: "short" });
}

/** Grobe Passwortstärke über die geschätzte Entropie. */
export function passwordStrength(pw: string): { bits: number; label: string; level: 0 | 1 | 2 | 3 } {
  let pool = 0;
  if (/[a-z]/.test(pw)) pool += 26;
  if (/[A-Z]/.test(pw)) pool += 26;
  if (/[0-9]/.test(pw)) pool += 10;
  if (/[^a-zA-Z0-9]/.test(pw)) pool += 33;
  const unique = new Set(pw).size;
  const bits = Math.round(Math.min(pw.length, unique * 1.5) * Math.log2(Math.max(pool, 1)));
  if (pw.length === 0) return { bits, label: "", level: 0 };
  if (bits < 45) return { bits, label: "schwach", level: 1 };
  if (bits < 70) return { bits, label: "mittel", level: 2 };
  return { bits, label: "stark", level: 3 };
}
