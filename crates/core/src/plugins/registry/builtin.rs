//! Built-in plugin instantiation.
//!
//! Contains the canonical list of all built-in plugins, categorized by domain.

use super::super::{
    Plugin, angular::AngularPlugin, astro::AstroPlugin, ava::AvaPlugin, babel::BabelPlugin,
    biome::BiomePlugin, bun::BunPlugin, c8::C8Plugin, capacitor::CapacitorPlugin,
    changesets::ChangesetsPlugin, commitizen::CommitizenPlugin, commitlint::CommitlintPlugin,
    convex::ConvexPlugin, cspell::CspellPlugin, cucumber::CucumberPlugin, cypress::CypressPlugin,
    dependency_cruiser::DependencyCruiserPlugin, docusaurus::DocusaurusPlugin,
    drizzle::DrizzlePlugin, electron::ElectronPlugin, eslint::EslintPlugin, expo::ExpoPlugin,
    expo_router::ExpoRouterPlugin, gatsby::GatsbyPlugin, graphql_codegen::GraphqlCodegenPlugin,
    hardhat::HardhatPlugin, husky::HuskyPlugin, i18next::I18nextPlugin, jest::JestPlugin,
    karma::KarmaPlugin, knex::KnexPlugin, kysely::KyselyPlugin, lefthook::LefthookPlugin,
    lint_staged::LintStagedPlugin, lit::LitPlugin, markdownlint::MarkdownlintPlugin,
    mocha::MochaPlugin, msw::MswPlugin, nestjs::NestJsPlugin, next_intl::NextIntlPlugin,
    nextjs::NextJsPlugin, nitro::NitroPlugin, nodemon::NodemonPlugin, nuxt::NuxtPlugin,
    nx::NxPlugin, nyc::NycPlugin, openapi_ts::OpenapiTsPlugin, oxlint::OxlintPlugin,
    pandacss::PandaCssPlugin, parcel::ParcelPlugin, playwright::PlaywrightPlugin, plop::PlopPlugin,
    pm2::Pm2Plugin, pnpm::PnpmPlugin, postcss::PostCssPlugin, prettier::PrettierPlugin,
    prisma::PrismaPlugin, qwik::QwikPlugin, react_native::ReactNativePlugin,
    react_router::ReactRouterPlugin, relay::RelayPlugin, remark::RemarkPlugin, remix::RemixPlugin,
    rolldown::RolldownPlugin, rollup::RollupPlugin, rsbuild::RsbuildPlugin, rspack::RspackPlugin,
    sanity::SanityPlugin, semantic_release::SemanticReleasePlugin, sentry::SentryPlugin,
    simple_git_hooks::SimpleGitHooksPlugin, storybook::StorybookPlugin, stylelint::StylelintPlugin,
    sveltekit::SvelteKitPlugin, svgo::SvgoPlugin, svgr::SvgrPlugin, swc::SwcPlugin,
    syncpack::SyncpackPlugin, tailwind::TailwindPlugin, tanstack_router::TanstackRouterPlugin,
    tap::TapPlugin, tsd::TsdPlugin, tsdown::TsdownPlugin, tsup::TsupPlugin,
    turborepo::TurborepoPlugin, typedoc::TypedocPlugin, typeorm::TypeormPlugin,
    typescript::TypeScriptPlugin, unocss::UnoCssPlugin, vite::VitePlugin,
    vitepress::VitePressPlugin, vitest::VitestPlugin, webdriverio::WebdriverioPlugin,
    webpack::WebpackPlugin, wrangler::WranglerPlugin,
};

/// Create all built-in plugin instances, categorized by domain.
pub fn create_builtin_plugins() -> Vec<Box<dyn Plugin>> {
    vec![
        // Frameworks
        Box::new(NextJsPlugin),
        Box::new(NuxtPlugin),
        Box::new(RemixPlugin),
        Box::new(AstroPlugin),
        Box::new(AngularPlugin),
        Box::new(ReactRouterPlugin),
        Box::new(TanstackRouterPlugin),
        Box::new(ReactNativePlugin),
        Box::new(ExpoPlugin),
        Box::new(ExpoRouterPlugin),
        Box::new(NestJsPlugin),
        Box::new(DocusaurusPlugin),
        Box::new(GatsbyPlugin),
        Box::new(SvelteKitPlugin),
        Box::new(NitroPlugin),
        Box::new(CapacitorPlugin),
        Box::new(SanityPlugin),
        Box::new(VitePressPlugin),
        Box::new(NextIntlPlugin),
        Box::new(RelayPlugin),
        Box::new(ElectronPlugin),
        Box::new(I18nextPlugin),
        Box::new(QwikPlugin),
        Box::new(ConvexPlugin),
        Box::new(LitPlugin),
        // Bundlers
        Box::new(VitePlugin),
        Box::new(WebpackPlugin),
        Box::new(RollupPlugin),
        Box::new(RolldownPlugin),
        Box::new(RspackPlugin),
        Box::new(RsbuildPlugin),
        Box::new(TsupPlugin),
        Box::new(TsdownPlugin),
        Box::new(ParcelPlugin),
        // Testing
        Box::new(VitestPlugin),
        Box::new(JestPlugin),
        Box::new(PlaywrightPlugin),
        Box::new(CypressPlugin),
        Box::new(MochaPlugin),
        Box::new(AvaPlugin),
        Box::new(TapPlugin),
        Box::new(TsdPlugin),
        Box::new(StorybookPlugin),
        Box::new(KarmaPlugin),
        Box::new(CucumberPlugin),
        Box::new(WebdriverioPlugin),
        // Linting & formatting
        Box::new(EslintPlugin),
        Box::new(BiomePlugin),
        Box::new(StylelintPlugin),
        Box::new(PrettierPlugin),
        Box::new(OxlintPlugin),
        Box::new(MarkdownlintPlugin),
        Box::new(CspellPlugin),
        Box::new(RemarkPlugin),
        // Transpilation & language
        Box::new(TypeScriptPlugin),
        Box::new(BabelPlugin),
        Box::new(SwcPlugin),
        // CSS
        Box::new(TailwindPlugin),
        Box::new(PostCssPlugin),
        Box::new(UnoCssPlugin),
        Box::new(PandaCssPlugin),
        // Database & ORM
        Box::new(PrismaPlugin),
        Box::new(DrizzlePlugin),
        Box::new(KnexPlugin),
        Box::new(TypeormPlugin),
        Box::new(KyselyPlugin),
        // Monorepo
        Box::new(TurborepoPlugin),
        Box::new(NxPlugin),
        Box::new(ChangesetsPlugin),
        Box::new(SyncpackPlugin),
        // CI/CD & release
        Box::new(CommitlintPlugin),
        Box::new(CommitizenPlugin),
        Box::new(SemanticReleasePlugin),
        // Blockchain
        Box::new(HardhatPlugin),
        // Deployment
        Box::new(WranglerPlugin),
        Box::new(SentryPlugin),
        // Git hooks
        Box::new(HuskyPlugin),
        Box::new(LintStagedPlugin),
        Box::new(LefthookPlugin),
        Box::new(SimpleGitHooksPlugin),
        // Media & assets
        Box::new(SvgoPlugin),
        Box::new(SvgrPlugin),
        // Code generation & docs
        Box::new(GraphqlCodegenPlugin),
        Box::new(TypedocPlugin),
        Box::new(OpenapiTsPlugin),
        Box::new(PlopPlugin),
        // Coverage
        Box::new(C8Plugin),
        Box::new(NycPlugin),
        // Other tools
        Box::new(MswPlugin),
        Box::new(NodemonPlugin),
        Box::new(Pm2Plugin),
        Box::new(DependencyCruiserPlugin),
        // Package managers
        Box::new(PnpmPlugin),
        // Runtime
        Box::new(BunPlugin),
    ]
}
