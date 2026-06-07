import type {
  AnalyzeResponse,
  ConceptTag,
  ReaderResponse,
  UploadResponse,
  UserBaseline
} from "./types";

const API_BASE_URL = import.meta.env.VITE_API_BASE_URL ?? "";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE_URL}${path}`, init);
  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: response.statusText }));
    throw new Error(error.error ?? "Request failed");
  }
  return response.json() as Promise<T>;
}

export async function uploadDocument(file: File): Promise<UploadResponse> {
  const form = new FormData();
  form.append("file", file);
  return request<UploadResponse>("/api/documents", {
    method: "POST",
    body: form
  });
}

export async function getConcepts(documentId: string): Promise<ConceptTag[]> {
  return request<ConceptTag[]>(`/api/documents/${documentId}/concepts`);
}

export async function setBaseline(
  documentId: string,
  baseline: UserBaseline
): Promise<UserBaseline> {
  return request<UserBaseline>(`/api/documents/${documentId}/baseline`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(baseline)
  });
}

export async function analyzeDocument(documentId: string): Promise<AnalyzeResponse> {
  return request<AnalyzeResponse>(`/api/documents/${documentId}/analyze`, {
    method: "POST"
  });
}

export async function getReader(documentId: string): Promise<ReaderResponse> {
  return request<ReaderResponse>(`/api/documents/${documentId}/reader`);
}

export function pdfUrl(documentId: string): string {
  return `${API_BASE_URL}/api/documents/${documentId}/pdf`;
}

