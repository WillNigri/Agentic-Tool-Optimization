import { invoke } from "@tauri-apps/api/core";

export interface InputRecord {
  id: string;
  slug: string;
  name: string;
  content: string;
  kind: string;
  tags: string[];
  createdAt: string;
  updatedAt: string;
}

export function list_inputs(): Promise<InputRecord[]> {
  return invoke<InputRecord[]>("list_inputs");
}

export function get_input(id: string): Promise<InputRecord> {
  return invoke<InputRecord>("get_input", { id });
}
