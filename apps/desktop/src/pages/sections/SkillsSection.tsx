import { lazy } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, Server, ShoppingBag, Briefcase } from "lucide-react";
import SectionTabs, { type TabDef } from "./SectionTabs";

const CustomizeOverview = lazy(() => import("./CustomizeOverview"));
const SkillsManager = lazy(() => import("@/components/SkillsManager"));
const McpManager = lazy(() => import("@/components/McpManager"));
const Marketplace = lazy(() => import("@/components/MarketplaceGrid"));

export default function SkillsSection() {
  const { t } = useTranslation();
  const tabs: TabDef[] = [
    {
      id: "overview",
      label: t("subnav.skillsOverview", "Overview"),
      icon: Briefcase,
      Component: CustomizeOverview,
    },
    {
      id: "skills",
      label: t("subnav.skillsSkills", "Skills"),
      icon: Sparkles,
      Component: SkillsManager,
    },
    {
      id: "mcps",
      label: t("subnav.skillsMcps", "MCPs"),
      icon: Server,
      Component: McpManager,
    },
    {
      id: "marketplace",
      label: t("subnav.skillsMarketplace", "Marketplace"),
      icon: ShoppingBag,
      Component: Marketplace,
    },
  ];
  return <SectionTabs storageKey="ato.subtab.skills" tabs={tabs} defaultTab="overview" />;
}
