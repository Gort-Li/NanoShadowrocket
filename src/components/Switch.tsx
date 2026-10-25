import { cn } from "../lib/cn";

export default function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative h-[26px] w-[44px] shrink-0 rounded-full border transition-colors duration-200",
        checked
          ? "border-accent/40 bg-accent shadow-[0_0_16px_rgba(10,132,255,0.35)]"
          : "border-white/10 bg-white/[0.08]",
        disabled ? "cursor-not-allowed opacity-40" : "cursor-pointer"
      )}
    >
      <span
        className={cn(
          "absolute top-[2px] h-[20px] w-[20px] rounded-full bg-white shadow-[0_1px_4px_rgba(0,0,0,0.45)]",
          "transition-[transform,width] duration-200 ease-spring",
          checked ? "translate-x-[20px]" : "translate-x-[2px]"
        )}
      />
    </button>
  );
}
