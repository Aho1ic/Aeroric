export function DockerIcon({ size = 18 }: { size?: number }) {
  const blocks = [
    [13.8, 1.4],
    [9.6, 5.4],
    [13.8, 5.4],
    [18, 5.4],
    [5.4, 9.4],
    [9.6, 9.4],
    [13.8, 9.4],
    [18, 9.4],
    [22.2, 9.4],
    [1.2, 13.4],
    [5.4, 13.4],
    [9.6, 13.4],
    [13.8, 13.4],
  ];

  return (
    <svg
      aria-hidden="true"
      data-testid="docker-logo-icon"
      width={size}
      height={size}
      viewBox="0 0 32 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      style={{ display: "block" }}
    >
      {blocks.map(([x, y]) => (
        <rect
          key={`${x}-${y}`}
          data-docker-block="true"
          x={x}
          y={y}
          width="3.6"
          height="3.4"
          rx="0.35"
          fill="currentColor"
        />
      ))}
      <path
        data-docker-hull="true"
        fill="currentColor"
        d="M1.8 16.6h20.4c2.8 0 5.1-.7 6.8-2.1.2-.2.6-.1.7.2.1.6-.1 1.4-.6 2.2-1.7 2.8-5.6 5.5-12.4 5.5H9.1c-3.8 0-6.5-1.9-7.8-5.2-.1-.3.1-.6.5-.6Z"
      />
      <path
        data-docker-whale="true"
        fill="currentColor"
        d="M24 13.5c-.8-1.7-.5-3.8.8-5.5.1-.2.4-.2.6-.1 1.5.9 2.4 2.3 2.6 4 1-.5 2.3-.5 3.4.2.2.1.3.4.2.6-.7 1.4-2 2.4-3.7 2.9-1.3.4-2.6.3-3.9-.1-.7-.2-.8-1.3 0-2Z"
      />
      <circle cx="6.4" cy="18.1" r="0.75" fill="white" opacity="0.92" />
    </svg>
  );
}
