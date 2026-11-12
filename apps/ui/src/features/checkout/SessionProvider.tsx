/**
 * SessionProvider (spec 001c-1, amendment 4): the abstraction that
 * supplies the actor (user, terminal, shift) to the sell screen.
 *
 * It loads the session + till status once on mount through the
 * `CoreClient`. In dev a feature-gated auto-session fills it; real
 * sign-in/lock is spec 001d — until then a build without a session
 * renders the no-session state (never a fake signed-in user).
 */
import { createContext, useContext, useEffect, useMemo, useState } from "react";
import type { ReactElement, ReactNode } from "react";
import type { SessionDto, StatusDto } from "@shopcore/protocol-ts";
import type { CoreClient } from "../../lib/core-client";

export interface SessionValue {
  /** The session, or `null` when no session is available. */
  session: SessionDto | null;
  /** The till status, or `null` until it loads. */
  status: StatusDto | null;
  /** True while the session/status are loading. */
  loading: boolean;
  /** True when the load failed (retry via `refresh`). */
  error: boolean;
  /** Re-load the session + status. */
  refresh: () => void;
}

const SessionContext = createContext<SessionValue | null>(null);

export function useSession(): SessionValue {
  const value = useContext(SessionContext);
  if (value === null) {
    throw new Error("useSession must be used inside a SessionProvider");
  }
  return value;
}

export function SessionProvider({
  client,
  children,
}: {
  client: CoreClient;
  children: ReactNode;
}): ReactElement {
  const [session, setSession] = useState<SessionDto | null>(null);
  const [status, setStatus] = useState<StatusDto | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let cancelled = false;
    Promise.all([client.getSession(), client.getStatus()])
      .then(([nextSession, nextStatus]) => {
        if (cancelled) {
          return;
        }
        setSession(nextSession);
        setStatus(nextStatus);
        setLoading(false);
      })
      .catch(() => {
        if (cancelled) {
          return;
        }
        setError(true);
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [client, nonce]);

  const value = useMemo<SessionValue>(
    () => ({
      session,
      status,
      loading,
      error,
      refresh: () => {
        // The loading/error reset lives here (an event handler), not
        // in the effect — sync setState in an effect is a cascading
        // render.
        setLoading(true);
        setError(false);
        setNonce((n) => n + 1);
      },
    }),
    [session, status, loading, error],
  );

  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}
