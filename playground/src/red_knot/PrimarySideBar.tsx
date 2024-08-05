import { FileIcon, SettingsIcon } from "../shared/Icons";
import SideBar, { SideBarEntry } from "../shared/SideBar";

type Tool = "Settings" | "Source";

type SideBarProps = {
  selected: Tool;
  onSelectTool(tool: Tool): void;
};

export default function PrimarySideBar({
  selected,
  onSelectTool,
}: SideBarProps) {
  return (
    <SideBar position="left">
      <SideBarEntry
        title="Source"
        position={"left"}
        onClick={() => onSelectTool("Source")}
        selected={selected == "Source"}
      >
        <FileIcon />
      </SideBarEntry>
    </SideBar>
  );
}
