/** The recurring footer buttons of the share and receive windows. */
import { t } from "../i18n";
import { Button } from "./Button";

export function CancelButton({ onClick }: { onClick: () => void }) {
  return <Button onClick={onClick}>{t("action.cancel")}</Button>;
}

export function CloseButton({ onClick }: { onClick: () => void }) {
  return <Button onClick={onClick}>{t("action.close")}</Button>;
}

/** "Neuen Code erstellen" after the code expired. */
export function RenewButton({ onClick }: { onClick: () => void }) {
  return (
    <Button variant="primary" onClick={onClick}>
      {t("action.renew")}
    </Button>
  );
}
