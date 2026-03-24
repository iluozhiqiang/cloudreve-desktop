import {
  Alert,
  Box,
  Button,
  Chip,
  Stack,
  Tooltip,
  Typography,
} from "@mui/material";
import {
  CheckCircleOutline as CheckCircleOutlineIcon,
  ErrorOutline as ErrorOutlineIcon,
  FolderOpen as FolderOpenIcon,
  Refresh as RefreshIcon,
  WarningAmber as WarningAmberIcon,
} from "@mui/icons-material";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { DriveInfo } from "../settings/types";
import { getFriendlyTaskError } from "./taskErrorMessage";
import type { TaskRecord, TaskWithProgress } from "./types";

export type TaskListFilterMode = "all" | "active" | "failed";

interface DriveStatusOverviewProps {
  drives: DriveInfo[];
  selectedDrive: string | null;
  activeTasks: TaskWithProgress[];
  finishedTasks: TaskRecord[];
  onOpenFolder: (path: string) => void;
  onReauthorize: (drive: DriveInfo) => void;
  taskListFilter: TaskListFilterMode;
  /** `all` sets filter to show everything; active/failed toggle off when clicked again. */
  onTaskListFilterChipClick: (kind: "all" | "active" | "failed") => void;
}

function getStatusColor(status: DriveInfo["status"]) {
  switch (status) {
    case "active":
      return "success";
    case "event_push_lost":
      return "warning";
    case "credential_expired":
      return "error";
    default:
      return "default";
  }
}

function getStatusLabel(status: DriveInfo["status"], t: ReturnType<typeof useTranslation>["t"]) {
  switch (status) {
    case "active":
      return t("settings.driveStatus.active");
    case "event_push_lost":
      return t("settings.driveStatus.eventPushLost");
    case "credential_expired":
      return t("settings.driveStatus.credentialExpired");
    default:
      return status;
  }
}

function getIssueMessage(drive: DriveInfo, t: ReturnType<typeof useTranslation>["t"]) {
  switch (drive.status) {
    case "credential_expired":
      return t(
        "popup.driveCredentialExpiredHint",
        "This drive needs reauthorization before sync can continue."
      );
    case "event_push_lost":
      return t(
        "popup.driveEventPushLostHint",
        "Real time push is unavailable right now. The app will rely more on polling."
      );
    default:
      return null;
  }
}

export default function DriveStatusOverview({
  drives,
  selectedDrive,
  activeTasks,
  finishedTasks,
  onOpenFolder,
  onReauthorize,
  taskListFilter,
  onTaskListFilterChipClick,
}: DriveStatusOverviewProps) {
  const { t } = useTranslation();

  const driveTaskStats = useMemo(() => {
    const stats = new Map<
      string,
      { active: number; failed: number; lastFailed?: TaskRecord }
    >();

    const ensure = (driveId: string) => {
      if (!stats.has(driveId)) {
        stats.set(driveId, { active: 0, failed: 0 });
      }
      return stats.get(driveId)!;
    };

    activeTasks.forEach((task) => {
      ensure(task.drive_id).active += 1;
    });

    finishedTasks.forEach((task) => {
      if (task.status !== "Failed" && task.status !== "Cancelled") {
        return;
      }
      const entry = ensure(task.drive_id);
      entry.failed += 1;
      if (
        task.status === "Failed" &&
        (!entry.lastFailed || task.updated_at > entry.lastFailed.updated_at)
      ) {
        entry.lastFailed = task;
      }
    });

    return stats;
  }, [activeTasks, finishedTasks]);

  const issueDrives = drives.filter((drive) => drive.status !== "active");
  const selectedDriveInfo = selectedDrive
    ? drives.find((drive) => drive.id === selectedDrive) ?? null
    : null;
  const activeTaskCount = activeTasks.length;
  const totalListedTaskCount = activeTaskCount + finishedTasks.length;
  const globalFailedCount = useMemo(() => {
    const failedLike = (task: TaskRecord) =>
      task.status === "Failed" || task.status === "Cancelled";
    return (
      activeTasks.filter((task) => failedLike(task)).length +
      finishedTasks.filter((task) => failedLike(task)).length
    );
  }, [activeTasks, finishedTasks]);

  if (drives.length === 0) {
    return null;
  }

  if (!selectedDriveInfo) {
    return (
      <Box sx={{ px: 2, py: 1.5, borderBottom: 1, borderColor: "divider" }}>
        <Stack direction="row" spacing={1}>
          <Box
            component="button"
            type="button"
            onClick={() => onTaskListFilterChipClick("all")}
            sx={{
              flex: 1,
              p: 1,
              borderRadius: 1.5,
              border: 1,
              borderColor:
                taskListFilter === "all" ? "primary.main" : "divider",
              bgcolor:
                taskListFilter === "all" ? "primary.main" : "action.hover",
              color: taskListFilter === "all" ? "primary.contrastText" : "text.primary",
              cursor: "pointer",
              textAlign: "left",
              font: "inherit",
            }}
          >
            <Typography
              variant="caption"
              color={taskListFilter === "all" ? "inherit" : "text.secondary"}
            >
              {t("popup.taskFilterAll", "All")}
            </Typography>
            <Typography variant="body2" fontWeight={600}>
              {totalListedTaskCount}
            </Typography>
          </Box>
          <Box
            component="button"
            type="button"
            onClick={() => onTaskListFilterChipClick("active")}
            sx={{
              flex: 1,
              p: 1,
              borderRadius: 1.5,
              border: 1,
              borderColor:
                taskListFilter === "active" ? "primary.main" : "divider",
              bgcolor:
                taskListFilter === "active" ? "primary.main" : "action.hover",
              color: taskListFilter === "active" ? "primary.contrastText" : "text.primary",
              cursor: "pointer",
              textAlign: "left",
              font: "inherit",
            }}
          >
            <Typography
              variant="caption"
              color={taskListFilter === "active" ? "inherit" : "text.secondary"}
            >
              {t("popup.activeTasks", "Active tasks")}
            </Typography>
            <Typography variant="body2" fontWeight={600}>
              {activeTaskCount}
            </Typography>
          </Box>
          <Box
            component="button"
            type="button"
            onClick={() => onTaskListFilterChipClick("failed")}
            sx={{
              flex: 1,
              p: 1,
              borderRadius: 1.5,
              border: 1,
              borderColor:
                taskListFilter === "failed" ? "error.main" : "divider",
              bgcolor:
                taskListFilter === "failed" ? "error.main" : "action.hover",
              color: taskListFilter === "failed" ? "error.contrastText" : "text.primary",
              cursor: "pointer",
              textAlign: "left",
              font: "inherit",
            }}
          >
            <Typography
              variant="caption"
              color={taskListFilter === "failed" ? "inherit" : "text.secondary"}
            >
              {t("popup.failedTasks", "Failed")}
            </Typography>
            <Typography variant="body2" fontWeight={600}>
              {globalFailedCount}
            </Typography>
          </Box>
        </Stack>
        {issueDrives.length > 0 && (
          <Alert severity="warning" sx={{ mt: 1.25, py: 0 }}>
            <Typography variant="caption">
              {t("popup.issueDrivesHint", {
                count: issueDrives.length,
                defaultValue: "{{count}} drive(s) need attention. Select a drive above to inspect it.",
              })}
            </Typography>
          </Alert>
        )}
      </Box>
    );
  }

  const stats = driveTaskStats.get(selectedDriveInfo.id) ?? { active: 0, failed: 0 };
  const issueMessage = getIssueMessage(selectedDriveInfo, t);

  return (
    <Box sx={{ px: 2, py: 1.5, borderBottom: 1, borderColor: "divider" }}>
      <Stack spacing={1}>
        <Box sx={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 1 }}>
          <Box sx={{ minWidth: 0 }}>
            <Typography variant="body2" fontWeight={600} noWrap>
              {selectedDriveInfo.name}
            </Typography>
            <Typography variant="caption" color="text.secondary" noWrap>
              {selectedDriveInfo.sync_path}
            </Typography>
          </Box>
          <Chip
            size="small"
            color={getStatusColor(selectedDriveInfo.status)}
            label={getStatusLabel(selectedDriveInfo.status, t)}
          />
        </Box>

        <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
          <Chip
            size="small"
            clickable
            onClick={() => onTaskListFilterChipClick("all")}
            variant={taskListFilter === "all" ? "filled" : "outlined"}
            color={taskListFilter === "all" ? "primary" : "default"}
            label={t("popup.taskFilterAll", "All")}
          />
          <Chip
            size="small"
            clickable
            onClick={() => onTaskListFilterChipClick("active")}
            variant={taskListFilter === "active" ? "filled" : "outlined"}
            color={taskListFilter === "active" ? "primary" : "default"}
            icon={
              stats.active > 0 ? (
                <RefreshIcon sx={{ fontSize: 16 }} />
              ) : (
                <CheckCircleOutlineIcon sx={{ fontSize: 16 }} />
              )
            }
            label={t("popup.activeTasksCount", {
              count: stats.active,
              defaultValue: "{{count}} active task(s)",
            })}
          />
          <Chip
            size="small"
            clickable
            onClick={() => onTaskListFilterChipClick("failed")}
            variant={taskListFilter === "failed" ? "filled" : "outlined"}
            color={taskListFilter === "failed" ? "error" : "default"}
            icon={<ErrorOutlineIcon sx={{ fontSize: 16 }} />}
            label={t("popup.failedTasksCount", {
              count: stats.failed,
              defaultValue: "{{count}} recent failure(s)",
            })}
          />
        </Stack>

        {issueMessage && (
          <Alert
            severity={selectedDriveInfo.status === "credential_expired" ? "error" : "warning"}
            sx={{ py: 0 }}
          >
            <Typography variant="caption">{issueMessage}</Typography>
          </Alert>
        )}

        {stats.lastFailed?.error && (
          <Alert severity="error" sx={{ py: 0 }}>
            <Tooltip title={stats.lastFailed.error} placement="top-start" enterDelay={300}>
              <Typography
                variant="caption"
                sx={{
                  wordBreak: "break-word",
                  display: "-webkit-box",
                  WebkitLineClamp: 3,
                  WebkitBoxOrient: "vertical",
                  overflow: "hidden",
                }}
              >
                {getFriendlyTaskError(stats.lastFailed.error, t)}
              </Typography>
            </Tooltip>
          </Alert>
        )}

        <Stack direction="row" spacing={1}>
          <Button
            size="small"
            variant="text"
            startIcon={<FolderOpenIcon />}
            onClick={() => onOpenFolder(selectedDriveInfo.sync_path)}
          >
            {t("settings.openFolder")}
          </Button>
          {selectedDriveInfo.status === "credential_expired" && (
            <Button
              size="small"
              variant="text"
              color="error"
              startIcon={<WarningAmberIcon />}
              onClick={() => onReauthorize(selectedDriveInfo)}
            >
              {t("settings.reauthorize")}
            </Button>
          )}
        </Stack>
      </Stack>
    </Box>
  );
}
