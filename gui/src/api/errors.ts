import type { ErrorBody } from "./types";

/**
 * The one error every API call fails with. `code` is the server's stable code (`unauthorized`,
 * `not_found`, `no_run`, `conflict`, `bad_request`, …) or, when no usable answer came back,
 * `network` (the server cannot be reached) or `bad_response`. `status` is 0 for `network`.
 * The message never contains the token.
 */
export class ApiError extends Error {
  readonly code: string;
  readonly status: number;
  readonly requestId: string | null;

  constructor(code: string, message: string, status: number, requestId: string | null = null) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.status = status;
    this.requestId = requestId;
  }

  /** The server answered 401: the token is missing, wrong or was replaced. */
  get isUnauthorized(): boolean {
    return this.status === 401;
  }
  /** Nothing answered at the address. */
  get isNetwork(): boolean {
    return this.code === "network";
  }
  /** The project has no run yet. */
  get isNoRun(): boolean {
    return this.code === "no_run";
  }
}

export function isApiError(value: unknown): value is ApiError {
  return value instanceof ApiError;
}

function isErrorBody(value: unknown): value is ErrorBody {
  if (typeof value !== "object" || value === null || !("error" in value)) return false;
  const e = (value as { error: unknown }).error;
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as { code?: unknown }).code === "string" &&
    typeof (e as { message?: unknown }).message === "string"
  );
}

/** Turn a non-2xx response into the shared error: the server's body when it sent one. */
export async function errorFromResponse(response: Response): Promise<ApiError> {
  let body: unknown;
  try {
    body = await response.json();
  } catch {
    body = undefined;
  }
  if (isErrorBody(body)) {
    const { code, message, request_id } = body.error;
    return new ApiError(code, message, response.status, request_id || null);
  }
  return new ApiError(
    "bad_response",
    `the server answered ${response.status} with something that is not an API error`,
    response.status,
  );
}
