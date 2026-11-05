export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
    this.name = "ApiError";
  }
}

export async function monitor<T>(path: string, key: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(`/api/monitor/${path}`, {
    headers: { Authorization: `Bearer ${key}` },
    cache: "no-store",
    signal,
  });
  const data = await response.json();
  // The readiness endpoint deliberately returns its structured state with 503.
  if (!response.ok && !(path === "health/ready" && typeof data.ready === "boolean")) {
    throw new ApiError(response.status, data.error ?? "The monitor could not complete this request.");
  }
  return data as T;
}
