// Bootstrap edits only this identity block; prose pages remain project-owned.
const generatedIdentity = {
  "name": "slingring",
  "strapline": "Launch ChatGPT tasks from local and remote agents, then get out of the way.",
  "description": "Launch ChatGPT tasks from local and remote agents, then get out of the way.",
  "repoUrl": "https://github.com/amxv/slingring",
  "footerText": "Launch ChatGPT tasks from local and remote agents, then get out of the way.",
  "distributionText": "Made for your Mac. Ready for your agents."
} as const;

export const siteConfig = {
  name: generatedIdentity.name,
  strapline: generatedIdentity.strapline,
  description: generatedIdentity.description,
  repoUrl: generatedIdentity.repoUrl,
  accentColor: "#b7410e",
  accentColorDark: "#f28c52",
  footerSections: [
    {
      title: generatedIdentity.name,
      text: generatedIdentity.footerText
    },
    {
      title: "Distribution",
      text: generatedIdentity.distributionText
    },
    {
      title: "Repository",
      linkPrefix: "Source: ",
      linkHref: generatedIdentity.repoUrl,
      linkLabel: generatedIdentity.repoUrl.replace("https://", "")
    }
  ]
} as const;

export const docCategories = ["Start here", "Guides", "Reference"] as const;

export const primaryNav = [
  { href: "/docs", label: "Docs" },
  { href: siteConfig.repoUrl, label: "GitHub", external: true }
];
