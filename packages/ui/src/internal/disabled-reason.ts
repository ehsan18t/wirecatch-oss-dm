/** The reason to show for a disabled control: only while it is disabled and not merely busy. */
export const blockedReason = (
  disabled: boolean | null | undefined,
  loading: boolean,
  reason: string | null | undefined,
): string | null => (disabled === true && !loading && reason ? reason : null);

/** What a control knows about its disabled reason, and the ids its caller already describes it with. */
export interface ReasonState {
  reason: string | null;
  reasonId: string;
  /** Busy with its own action: also aria-disabled, with nothing to describe. */
  busy?: boolean;
  /** The caller's own `aria-describedby`, kept beside the reason rather than replaced by it. */
  describedBy?: string | null | undefined;
}

/** The attributes a control disabled with a reason carries: aria-disabled, and a description that is the caller's ids followed by the reason's. */
export const reasonAttributes = ({ reason, reasonId, busy = false, describedBy }: ReasonState) => {
  const ids = [describedBy ?? '', reason === null ? '' : reasonId].filter((id) => id !== '');
  return {
    'aria-disabled': busy || reason !== null ? ('true' as const) : undefined,
    'aria-describedby': ids.length === 0 ? undefined : ids.join(' '),
  };
};
