import { ConfirmDialog as BaseConfirmDialog } from "@sdlc/ui/ui";
import { useTranslation } from "react-i18next";
import type { ComponentProps } from "react";

type ConfirmDialogProps = Omit<ComponentProps<typeof BaseConfirmDialog>, "onOpenChange" | "isPending"> & {
  onCancel: () => void;
  pending?: boolean;
};

export function ConfirmDialog({ onCancel, pending, closeOnConfirm = true, confirmLabel, ...props }: ConfirmDialogProps) {
  const { t } = useTranslation();
  return <BaseConfirmDialog {...props} confirmLabel={confirmLabel ?? t("common.delete")} pendingLabel={confirmLabel ?? t("common.delete")} closeOnConfirm={closeOnConfirm} isPending={pending} onOpenChange={(open) => { if (!open) onCancel(); }} />;
}
