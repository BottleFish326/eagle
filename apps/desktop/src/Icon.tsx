export type IconName =
  | "alert"
  | "check"
  | "chevron"
  | "close"
  | "folder"
  | "grid"
  | "image"
  | "library"
  | "minus"
  | "plus"
  | "refresh"
  | "search"
  | "star"
  | "tag"
  | "trash";

export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg
      aria-hidden="true"
      className="icon"
      fill="none"
      height={size}
      viewBox="0 0 24 24"
      width={size}
    >
      {paths[name]}
    </svg>
  );
}

const paths: Record<IconName, React.ReactNode> = {
  alert: (
    <>
      <path d="M12 8v5" />
      <path d="M12 17.25h.01" />
      <path d="M10.3 3.7 2.5 17.2A2 2 0 0 0 4.2 20h15.6a2 2 0 0 0 1.7-2.8L13.7 3.7a2 2 0 0 0-3.4 0Z" />
    </>
  ),
  check: <path d="m5 12 4.2 4.2L19 6.5" />,
  chevron: <path d="m9 18 6-6-6-6" />,
  close: (
    <>
      <path d="m6 6 12 12" />
      <path d="m18 6-12 12" />
    </>
  ),
  folder: (
    <path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2h7.5A2.5 2.5 0 0 1 21 9.5v7A2.5 2.5 0 0 1 18.5 19h-13A2.5 2.5 0 0 1 3 16.5Z" />
  ),
  grid: (
    <>
      <rect height="7" rx="1.5" width="7" x="3" y="3" />
      <rect height="7" rx="1.5" width="7" x="14" y="3" />
      <rect height="7" rx="1.5" width="7" x="3" y="14" />
      <rect height="7" rx="1.5" width="7" x="14" y="14" />
    </>
  ),
  image: (
    <>
      <rect height="16" rx="2.5" width="18" x="3" y="4" />
      <circle cx="8.5" cy="9" r="1.5" />
      <path d="m4 17 4.7-4.7a2 2 0 0 1 2.8 0L14 14.8l1.1-1.1a2 2 0 0 1 2.8 0L21 16.8" />
    </>
  ),
  library: (
    <>
      <path d="M4 4h5v16H4z" />
      <path d="M9 4h5v16H9" />
      <path d="m14.5 5.5 4-1 3.5 14-4 1z" />
    </>
  ),
  minus: <path d="M5 12h14" />,
  plus: (
    <>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </>
  ),
  refresh: (
    <>
      <path d="M20 7v5h-5" />
      <path d="M4 17v-5h5" />
      <path d="M18.3 9A7 7 0 0 0 6.2 6.3L4 9" />
      <path d="M5.7 15A7 7 0 0 0 17.8 17.7L20 15" />
    </>
  ),
  search: (
    <>
      <circle cx="10.8" cy="10.8" r="6.8" />
      <path d="m16 16 4.2 4.2" />
    </>
  ),
  star: (
    <path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9z" />
  ),
  tag: (
    <>
      <path d="M3 5.5V11l9.5 9.5 8-8L11 3H5.5A2.5 2.5 0 0 0 3 5.5Z" />
      <circle cx="7.5" cy="7.5" r="1" />
    </>
  ),
  trash: (
    <>
      <path d="M4 7h16" />
      <path d="M9 3h6l1 4H8z" />
      <path d="m6 7 1 14h10l1-14" />
      <path d="M10 11v6M14 11v6" />
    </>
  ),
};
