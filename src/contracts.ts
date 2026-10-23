export type Completeness = "partial" | "complete";
export type ConversationDateSource = "provider-updated" | "message-authored" | "provider-created";

export interface Attachment {
  id: string;
  name: string;
  mime_type: string;
  source_url?: string;
  data_url?: string;
  asset_path?: string;
  unavailable_reason?: string;
}

export interface Message {
  id: string;
  role: "user" | "assistant" | "system" | "tool";
  text: string;
  format: "markdown" | "plain";
  authored_at?: string;
  model?: string;
  attachments: Attachment[];
}

export interface Capture {
  schema_version: 1;
  provider: "chatgpt";
  conversation_id: string;
  source_url: string;
  title: string;
  conversation_created_at?: string;
  conversation_updated_at?: string;
  captured_at: string;
  capture_method: "manual-dom" | "automatic-dom" | "legacy-import";
  completeness: Completeness;
  warnings: string[];
  messages: Message[];
}

export interface ConversationSummary {
  id: string;
  conversation_id: string;
  provider: string;
  title: string;
  source_url: string;
  conversation_date?: string;
  date_source?: ConversationDateSource;
  captured_at: string;
  message_count: number;
  snapshot_count: number;
  completeness: Completeness;
  warnings: string[];
}

export interface Conversation {
  summary: ConversationSummary;
  capture: Capture;
  snapshot_id?: string;
}

export interface CaptureSnapshot {
  snapshot_id: string;
  captured_at: string;
  message_count: number;
  completeness: Completeness;
}

export interface SetupInfo {
  archive_path: string;
  extension_path: string;
  host_path: string;
  extension_id: string;
  native_host_name: string;
  registered_browsers: string[];
  platform: string;
  legacy_available: boolean;
}

export interface LegacyImportResult {
  imported: number;
  skipped: number;
  failed: number;
  errors: string[];
}

export interface GenericImportReceipt {
  format: "json" | "markdown" | "html";
  raw_sha256: string;
  raw_preserved: boolean;
  saved: { id: string; snapshot_id: string; unchanged: boolean; message_count: number } | null;
  errors: string[];
}

export interface ExportResult {
  path: string;
  markdown: string;
}

export interface AssetResult {
  mime_type: string;
  data_url: string;
}
