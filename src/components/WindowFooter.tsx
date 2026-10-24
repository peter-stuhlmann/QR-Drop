import type { ReactNode } from "react";

/** Bottom of a share/receive window: hint line, then the action buttons. Fixed heights keep the card still. */
export function WindowFooter({ hint, actions }: { hint?: ReactNode; actions: ReactNode }) {
  return (
    <>
      <div className="mt-2.5 flex min-h-[38px] shrink-0 items-center justify-center">{hint}</div>
      <div className="mt-2.5 flex h-[34px] shrink-0 items-center justify-center gap-2.5">
        {actions}
      </div>
    </>
  );
}
