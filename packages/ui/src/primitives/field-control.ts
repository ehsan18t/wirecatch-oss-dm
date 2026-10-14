/** What a Field hands its control: spread it onto the control (`<Input {...control} />`), so the label, the hint and the error point at it and an error marks it invalid. Every field control takes these three. */
export interface FieldControl {
  id: string;
  'aria-describedby': string | undefined;
  invalid: boolean;
}
