import { FileText, X } from "lucide-react";
import s from "../../styles";
import { formatCharCount } from "../../utils/format";

export interface PastedText {
  id: string;
  text: string;
}

export function TextAttachments({
  texts,
  onRemove,
}: {
  texts: PastedText[];
  onRemove: (id: string) => void;
}) {
  if (texts.length === 0) return null;

  return (
    <>
      {texts.map((item) => (
        <div key={item.id} style={s.textAttachmentChip}>
          <FileText size={18} style={s.textAttachmentIcon} />
          <span style={s.textAttachmentSize}>{formatCharCount(item.text.length)}</span>
          <button onClick={() => onRemove(item.id)} style={s.textAttachmentRemove}>
            <X size={10} />
          </button>
        </div>
      ))}
    </>
  );
}
