<script lang="ts">
  // A time of day typed as the app shows times, `07:30`, where the browser's own time field would bring its am/pm and clock picker. Up and Down step it by a quarter hour, round the clock.
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { Input } from '@wirecatch/ui';
  import { formatTime, parseTime } from '$lib/format';
  import { HOURS_PER_DAY, MINUTES_PER_HOUR, minutesOfDay } from '$lib/time';

  interface Props extends Pick<HTMLInputAttributes, 'id' | 'aria-describedby'> {
    value: string;
    label?: string;
    invalid?: boolean;
    onchange: (value: string) => void;
  }

  let { value, label, invalid = false, onchange, ...rest }: Props = $props();

  const STEP_MINUTES = 15;
  const DAY_MINUTES = HOURS_PER_DAY * MINUTES_PER_HOUR;

  function onkeydown(event: KeyboardEvent) {
    const direction = { ArrowUp: 1, ArrowDown: -1 }[event.key];
    const time = parseTime(value);
    if (direction === undefined || time === null) return;
    event.preventDefault();
    const minutes = minutesOfDay(time);
    // To the next quarter in that direction, so 07:10 goes to 07:15 or 07:00.
    const snapped =
      direction > 0
        ? Math.floor(minutes / STEP_MINUTES) * STEP_MINUTES + STEP_MINUTES
        : Math.ceil(minutes / STEP_MINUTES) * STEP_MINUTES - STEP_MINUTES;
    const next = (snapped + DAY_MINUTES) % DAY_MINUTES;
    onchange(
      formatTime({ hour: Math.floor(next / MINUTES_PER_HOUR), minute: next % MINUTES_PER_HOUR }),
    );
  }
</script>

<Input
  {...rest}
  {label}
  {invalid}
  {value}
  {onchange}
  {onkeydown}
  class="w-24 tabular-nums"
  inputmode="numeric"
  placeholder="hh:mm"
  autocomplete="off"
  spellcheck="false"
/>
