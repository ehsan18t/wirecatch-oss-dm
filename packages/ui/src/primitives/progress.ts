/** What a download's bar shows: moving data, a pause, a failure, a finish, or a wait in the queue. */
export type ProgressState = 'active' | 'paused' | 'error' | 'done' | 'queued';
