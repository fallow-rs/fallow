//! Built-in plugin instantiation.
//!
//! Contains the canonical list of all built-in plugins, categorized by domain.

use super::super::{
    Plugin, adonis::AdonisPlugin, angular::AngularPlugin, astro::AstroPlugin, ava::AvaPlugin,
    babel::BabelPlugin, biome::BiomePlugin, browser_extension::BrowserExtensionPlugin,
    bun::BunPlugin, c8::C8Plugin, capacitor::CapacitorPlugin, changesets::ChangesetsPlugin,
    commit_and_tag_version::CommitAndTagVersionPlugin, commitizen::CommitizenPlugin,
    commitlint::CommitlintPlugin, content_collections::ContentCollectionsPlugin,
    contentlayer::ContentlayerPlugin, convex::ConvexPlugin, cspell::CspellPlugin,
    cucumber::CucumberPlugin, cypress::CypressPlugin, danger::DangerPlugin,
    dependency_cruiser::DependencyCruiserPlugin, docusaurus::DocusaurusPlugin,
    drizzle::DrizzlePlugin, electron::ElectronPlugin, ember::EmberPlugin, eslint::EslintPlugin,
    expo::ExpoPlugin, expo_router::ExpoRouterPlugin, firebase::FirebasePlugin,
    fumadocs::FumadocsPlugin, gatsby::GatsbyPlugin, graphql_codegen::GraphqlCodegenPlugin,
    hardhat::HardhatPlugin, husky::HuskyPlugin, i18next::I18nextPlugin, ionic::IonicPlugin,
    jest::JestPlugin, k6::K6Plugin, karma::KarmaPlugin, knex::KnexPlugin, kysely::KyselyPlugin,
    lefthook::LefthookPlugin, lexical::LexicalPlugin, lint_staged::LintStagedPlugin,
    lit::LitPlugin, markdownlint::MarkdownlintPlugin, mintlify::MintlifyPlugin, mocha::MochaPlugin,
    msw::MswPlugin, napi_rs::NapiRsPlugin, nestjs::NestJsPlugin, next_intl::NextIntlPlugin,
    nextjs::NextJsPlugin, nitro::NitroPlugin, nodemon::NodemonPlugin, nuxt::NuxtPlugin,
    nx::NxPlugin, nyc::NycPlugin, obsidian::ObsidianPlugin, openapi_ts::OpenapiTsPlugin,
    opencode::OpenCodePlugin, opennext_cloudflare::OpenNextCloudflarePlugin, oxlint::OxlintPlugin,
    pandacss::PandaCssPlugin, parcel::ParcelPlugin, pinia::PiniaPlugin, pkg_utils::PkgUtilsPlugin,
    playwright::PlaywrightPlugin, plop::PlopPlugin, pm2::Pm2Plugin, pnpm::PnpmPlugin,
    postcss::PostCssPlugin, prettier::PrettierPlugin, prisma::PrismaPlugin, qwik::QwikPlugin,
    react_native::ReactNativePlugin, react_router::ReactRouterPlugin, redwoodsdk::RedwoodSdkPlugin,
    relay::RelayPlugin, remark::RemarkPlugin, remix::RemixPlugin, rolldown::RolldownPlugin,
    rollup::RollupPlugin, rsbuild::RsbuildPlugin, rspack::RspackPlugin, rspress::RspressPlugin,
    sanity::SanityPlugin, semantic_release::SemanticReleasePlugin, sentry::SentryPlugin,
    simple_git_hooks::SimpleGitHooksPlugin, storybook::StorybookPlugin, stryker::StrykerPlugin,
    stylelint::StylelintPlugin, supabase::SupabasePlugin, sveltekit::SvelteKitPlugin,
    svgo::SvgoPlugin, svgr::SvgrPlugin, swc::SwcPlugin, syncpack::SyncpackPlugin,
    tailwind::TailwindPlugin, tanstack_router::TanstackRouterPlugin, tap::TapPlugin,
    tsd::TsdPlugin, tsdown::TsdownPlugin, tsup::TsupPlugin, turborepo::TurborepoPlugin,
    typedoc::TypedocPlugin, typeorm::TypeormPlugin, typescript::TypeScriptPlugin,
    unocss::UnoCssPlugin, varlock::VarlockPlugin, velite::VelitePlugin, vercel::VercelPlugin,
    vite::VitePlugin, vitepress::VitePressPlugin, vitest::VitestPlugin, vscode::VscodePlugin,
    webdriverio::WebdriverioPlugin, webpack::WebpackPlugin, wrangler::WranglerPlugin,
    wuchale::WuchalePlugin, wxt::WxtPlugin,
};

macro_rules! push_plugins {
    ($plugins:expr, $($plugin:expr),+ $(,)?) => {
        $(
            $plugins.push(Box::new($plugin));
        )+
    };
}

/// Create all built-in plugin instances, categorized by domain.
pub fn create_builtin_plugins() -> Vec<Box<dyn Plugin>> {
    let mut plugins = Vec::new();
    add_framework_plugins(&mut plugins);
    add_content_and_platform_plugins(&mut plugins);
    add_build_and_test_plugins(&mut plugins);
    add_quality_and_language_plugins(&mut plugins);
    add_tooling_and_infra_plugins(&mut plugins);
    plugins
}

fn add_framework_plugins(plugins: &mut Vec<Box<dyn Plugin>>) {
    push_plugins!(
        plugins,
        NextJsPlugin,
        NuxtPlugin,
        PiniaPlugin,
        RemixPlugin,
        AstroPlugin,
        BrowserExtensionPlugin,
        WxtPlugin,
        AngularPlugin,
        ReactRouterPlugin,
        RedwoodSdkPlugin,
        TanstackRouterPlugin,
        ReactNativePlugin,
        ExpoPlugin,
        ExpoRouterPlugin,
        FirebasePlugin,
        NestJsPlugin,
        AdonisPlugin,
        DocusaurusPlugin,
        GatsbyPlugin,
        SvelteKitPlugin,
        NitroPlugin,
        CapacitorPlugin,
        IonicPlugin,
    );
}

fn add_content_and_platform_plugins(plugins: &mut Vec<Box<dyn Plugin>>) {
    push_plugins!(
        plugins,
        SanityPlugin,
        SupabasePlugin,
        VitePressPlugin,
        RspressPlugin,
        NextIntlPlugin,
        RelayPlugin,
        ElectronPlugin,
        I18nextPlugin,
        QwikPlugin,
        ConvexPlugin,
        LitPlugin,
        LexicalPlugin,
        ObsidianPlugin,
        ContentCollectionsPlugin,
        ContentlayerPlugin,
        FumadocsPlugin,
        MintlifyPlugin,
        VelitePlugin,
        EmberPlugin,
    );
}

fn add_build_and_test_plugins(plugins: &mut Vec<Box<dyn Plugin>>) {
    push_plugins!(
        plugins,
        VitePlugin,
        VscodePlugin,
        WebpackPlugin,
        RollupPlugin,
        RolldownPlugin,
        RspackPlugin,
        RsbuildPlugin,
        TsupPlugin,
        TsdownPlugin,
        PkgUtilsPlugin,
        ParcelPlugin,
        VitestPlugin,
        JestPlugin,
        PlaywrightPlugin,
        CypressPlugin,
        MochaPlugin,
        AvaPlugin,
        TapPlugin,
        TsdPlugin,
        K6Plugin,
        StorybookPlugin,
        StrykerPlugin,
        KarmaPlugin,
        CucumberPlugin,
        WebdriverioPlugin,
    );
}

fn add_quality_and_language_plugins(plugins: &mut Vec<Box<dyn Plugin>>) {
    push_plugins!(
        plugins,
        EslintPlugin,
        BiomePlugin,
        StylelintPlugin,
        PrettierPlugin,
        OxlintPlugin,
        MarkdownlintPlugin,
        CspellPlugin,
        RemarkPlugin,
        TypeScriptPlugin,
        BabelPlugin,
        SwcPlugin,
        TailwindPlugin,
        PostCssPlugin,
        UnoCssPlugin,
        PandaCssPlugin,
        PrismaPlugin,
        DrizzlePlugin,
        KnexPlugin,
        TypeormPlugin,
        KyselyPlugin,
    );
}

fn add_tooling_and_infra_plugins(plugins: &mut Vec<Box<dyn Plugin>>) {
    push_plugins!(
        plugins,
        TurborepoPlugin,
        NxPlugin,
        ChangesetsPlugin,
        SyncpackPlugin,
        CommitlintPlugin,
        CommitizenPlugin,
        CommitAndTagVersionPlugin,
        SemanticReleasePlugin,
        DangerPlugin,
        HardhatPlugin,
        VercelPlugin,
        WranglerPlugin,
        OpenNextCloudflarePlugin,
        SentryPlugin,
        HuskyPlugin,
        LintStagedPlugin,
        LefthookPlugin,
        SimpleGitHooksPlugin,
        SvgoPlugin,
        SvgrPlugin,
        GraphqlCodegenPlugin,
        TypedocPlugin,
        OpenapiTsPlugin,
        PlopPlugin,
        C8Plugin,
        NycPlugin,
        MswPlugin,
        NapiRsPlugin,
        OpenCodePlugin,
        NodemonPlugin,
        Pm2Plugin,
        DependencyCruiserPlugin,
        WuchalePlugin,
        VarlockPlugin,
        PnpmPlugin,
        BunPlugin,
    );
}
