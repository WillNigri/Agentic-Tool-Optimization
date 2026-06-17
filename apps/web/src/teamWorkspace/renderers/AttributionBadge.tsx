// Model A — shared-view attribution pill: "who ran this on which machine".
//
// member_id is the cloud user id (users.id) of the signed-in teammate who
// fired the seat; machine_id is the stable per-install UUID from the
// originating desktop/CLI. Both are opaque UUIDs in the snapshot — the
// snapshot doesn't carry a name lookup — so we render a short prefix with the
// full id on hover. Resolving member_id → display name is a cloud-side
// follow-up (the workspace API knows the member roster); the short id is
// already enough for a teammate to tell two members / two machines apart in
// the shared war-room.
//
// Renders nothing when neither id is present (pre-attribution rows, or a
// purely-local run where the operator wasn't signed in).

interface AttributionBadgeProps {
  memberId?: string | null;
  machineId?: string | null;
  className?: string;
}

function shortId(id: string): string {
  // UUIDs are 36 chars; show the leading block so distinct ids stay visually
  // distinct without dominating the card. Non-UUID ids just get truncated.
  return id.length > 8 ? id.slice(0, 8) : id;
}

export default function AttributionBadge({
  memberId,
  machineId,
  className = '',
}: AttributionBadgeProps) {
  if (!memberId && !machineId) return null;

  const titleParts = [
    memberId ? `member ${memberId}` : null,
    machineId ? `machine ${machineId}` : null,
  ].filter(Boolean);

  return (
    <span
      title={titleParts.join(' · ')}
      className={[
        'inline-flex items-center gap-1 rounded-full px-2 py-0.5',
        'bg-[#16161e] border border-[#2a2a3a] text-[10px] font-medium text-[#e8e8f0]',
        className,
      ].join(' ')}
    >
      {memberId && (
        <>
          <span aria-hidden>👥</span>
          <span className="font-mono">{shortId(memberId)}</span>
        </>
      )}
      {machineId && (
        <>
          {memberId && <span className="text-[#8888a0]" aria-hidden>·</span>}
          <span aria-hidden>🖥️</span>
          <span className="font-mono text-[#8888a0]">{shortId(machineId)}</span>
        </>
      )}
    </span>
  );
}
