export function DockerIcon({ size = 18 }: { size?: number }) {
  return (
    <svg
      aria-hidden="true"
      data-testid="docker-logo-icon"
      width={size}
      height={size}
      viewBox="0 0 32 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      style={{ display: "block", color: "#2496ED" }}
    >
      <path
        fill="currentColor"
        d="M13.3 2.5h3.8v3.7h-3.8V2.5Zm-4.5 4.4h3.8v3.7H8.8V6.9Zm4.5 0h3.8v3.7h-3.8V6.9Zm4.5 0h3.8v3.7h-3.8V6.9ZM4.3 11.3h3.8V15H4.3v-3.7Zm4.5 0h3.8V15H8.8v-3.7Zm4.5 0h3.8V15h-3.8v-3.7Zm4.5 0h3.8V15h-3.8v-3.7Z"
      />
      <path
        fill="currentColor"
        d="M29.6 10.9c-.9-.6-2.2-.7-3.2-.3-.2-1.2-.9-2.2-2-2.9l-.7-.4-.4.7c-.5.9-.7 2.3-.3 3.4.2.6.5 1.1.9 1.5-.8.4-2.1.4-2.8.4H2.1l-.1.8c-.2 2 .4 3.6 1.7 4.8 1.2 1.1 3.1 1.7 5.4 1.7 5.1 0 8.8-1.9 11.3-5.7h.4c3.1 0 5.6-.9 7.1-2.6.4-.4.7-.9.9-1.4l.2-.6-.4-.4Z"
      />
      <path fill="white" d="M6.1 16.2a.9.9 0 1 0 0 1.8.9.9 0 0 0 0-1.8Z" opacity="0.9" />
    </svg>
  );
}
