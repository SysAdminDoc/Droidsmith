import { useState } from "react";

export function useCommandPalette() {
  const [open, setOpen] = useState(false);

  return { open, setOpen };
}
