// Test-only service: no credentials, network, game processes or persistent user data.
#include <QJsonArray>
#include <QJsonDocument>
#include <QtGui>
#include <iostream>
static QString key(const char *s) {
    return QString::fromLatin1(s);
}
static QString png(const QImage &image) {
    QByteArray data;
    QBuffer buffer(&data);
    buffer.open(QIODevice::WriteOnly);
    image.save(&buffer, "PNG");
    return key("data:image/png;base64,") + QString::fromLatin1(data.toBase64());
}
static QImage skin(QColor coat) {
    QImage image(64, 64, QImage::Format_ARGB32);
    image.fill(Qt::transparent);
    QPainter p(&image);
    auto fill = [&](int x, int y, int w, int h, QColor c) { p.fillRect(x, y, w, h, c); };
    fill(0, 0, 32, 16, QColor(19, 28, 26));
    fill(8, 8, 8, 8, QColor(240, 195, 165));
    fill(8, 8, 8, 2, QColor(14, 25, 24));
    fill(8, 10, 2, 1, QColor(14, 25, 24));
    fill(14, 10, 2, 1, QColor(14, 25, 24));
    fill(9, 11, 2, 2, Qt::white);
    fill(13, 11, 2, 2, Qt::white);
    fill(10, 11, 1, 2, QColor(16, 28, 25));
    fill(13, 11, 1, 2, QColor(16, 28, 25));
    fill(16, 16, 24, 16, coat);
    fill(23, 20, 2, 12, Qt::white);
    fill(24, 21, 1, 8, QColor(27, 161, 209));
    fill(20, 31, 8, 1, QColor(211, 158, 49));
    fill(40, 16, 16, 16, coat);
    fill(32, 48, 16, 16, coat);
    fill(44, 20, 4, 2, QColor(227, 170, 35));
    fill(36, 52, 4, 2, QColor(227, 170, 35));
    fill(44, 30, 4, 2, QColor(240, 195, 165));
    fill(36, 62, 4, 2, QColor(240, 195, 165));
    fill(0, 16, 16, 16, QColor(203, 205, 196));
    fill(16, 48, 16, 16, QColor(203, 205, 196));
    for (int y = 20; y < 31; ++y)
        for (int x = 4; x < 8; ++x)
            if ((x / 2 + y / 2) % 2 == 0)
                fill(x, y, 1, 1, QColor(172, 178, 172));
    for (int y = 52; y < 63; ++y)
        for (int x = 20; x < 24; ++x)
            if ((x / 2 + y / 2) % 2 == 0)
                fill(x, y, 1, 1, QColor(172, 178, 172));
    fill(4, 31, 4, 1, QColor(217, 163, 26));
    fill(20, 63, 4, 1, QColor(217, 163, 26));
    return image;
}
static QString icon(QColor color, const QString &text) {
    QImage image(64, 64, QImage::Format_ARGB32);
    image.fill(color);
    QPainter p(&image);
    p.setPen(QColor(26, 39, 36));
    QFont font(key("Segoe UI"));
    font.setPixelSize(30);
    font.setBold(true);
    p.setFont(font);
    p.drawText(image.rect(), Qt::AlignCenter, text);
    return png(image);
}
int main(int argc, char **argv) {
    qputenv("QT_QPA_PLATFORM", "offscreen");
    QGuiApplication app(argc, argv);
    for (const auto &font : {key("segoeui.ttf"), key("segoeuib.ttf")})
        QFontDatabase::addApplicationFont(qEnvironmentVariable("WINDIR") + key("/Fonts/") + font);
    const auto blue = skin(QColor(0, 137, 203));
    const QString skinUrl = png(blue), head = png(blue.copy(8, 8, 8, 8)),
                  fo = icon(QColor(219, 211, 172), key("FO")),
                  sodium = icon(QColor(111, 198, 83), key("S+"));
    bool loggedIn = qEnvironmentVariable("CK_TEST_NO_ACCOUNT") != key("1");
    QString active = key("fo"), account = key("account-1");
    QJsonArray requests;
    QJsonObject profile{
        {key("id"), key("default")},
        {key("gameDir"), key("C:/Users/Player/AppData/Roaming/CKLauncher/instances/fo")},
        {key("memoryMb"), 4096}};
    QJsonArray skins;
    for (int i = 0; i < 3; ++i)
        skins.append(QJsonObject{{key("id"), key("skin-") + QString::number(i)},
                                 {key("name"), QString::fromUtf8(i == 0   ? "Нон-рп отряд"
                                                                 : i == 1 ? "Император"
                                                                          : "Зимний")},
                                 {key("dataUrl"), png(skin(i == 0   ? QColor(38, 42, 48)
                                                           : i == 1 ? QColor(225, 222, 188)
                                                                    : QColor(42, 55, 56)))},
                                 {key("isFavorite"), i < 2},
                                 {key("isActive"), false}});
    QJsonArray content;
    for (const auto &name : {key("Fabulously Optimized"), key("SodiumTranslations"),
                             key("Chat Reporting Helper"), key("Zoomify"), key("Sodium")})
        content.append(
            QJsonObject{{key("projectId"), name},
                        {key("projectType"), name == key("Fabulously Optimized") ? key("modpack")
                                             : name == key("Zoomify") || name == key("Sodium")
                                                 ? key("mod")
                                                 : key("resourcepack")},
                        {key("title"), name},
                        {key("filename"), name + key(".jar")},
                        {key("enabled"), true},
                        {key("iconUrl"), name == key("Fabulously Optimized") ? fo : sodium}});
    QJsonArray capes;
    for (int i = 0; i < 4; ++i) {
        QImage cape(64, 32, QImage::Format_ARGB32);
        cape.fill(Qt::transparent);
        QPainter p(&cape);
        p.fillRect(1, 1, 10, 16, QColor::fromHsv(i * 70, 200, 200));
        p.fillRect(4, 3, 4, 6, QColor(245, 227, 134));
        capes.append(QJsonObject{{key("id"), key("cape-") + QString::number(i)},
                                 {key("alias"), key("Cape ") + QString::number(i + 1)},
                                 {key("url"), png(cape)},
                                 {key("state"), i == 2 ? key("ACTIVE") : key("INACTIVE")}});
    }
    QString currentSkin = skinUrl;
    int launches = 0;
    int versionErrorAttempts = 0;
    QMap<QString, QString> failNext;
    bool holdInstall = false;
    QJsonObject heldInstall;
    std::string line;
    while (std::getline(std::cin, line)) {
        const auto request = QJsonDocument::fromJson(QByteArray::fromStdString(line)).object();
        const auto method = request.value(key("method")).toString();
        const auto params = request.value(key("params")).toObject();
        requests.append(request);
        QJsonValue result(QJsonValue::Null);
        bool known = true;
        auto event = [](const QString &name, const QJsonObject &data) {
            std::cout << QJsonDocument(QJsonObject{{key("event"), name}, {key("data"), data}})
                             .toJson(QJsonDocument::Compact)
                             .constData()
                      << std::endl;
        };
        if (method == key("hello"))
            result = QJsonObject{{key("protocolVersion"), 1}};
        else if (method == key("load_public_image"))
            result = params.value(key("url")).toString().contains(key("textures.minecraft.net"))
                         ? currentSkin.mid(22)
                         : head.mid(22);
        else if (method == key("launch_or_install")) {
            ++launches;
            const auto id = key("launch-") + QString::number(launches);
            result = id;
            if (launches == 1)
                event(key("launcher://error"),
                      {{key("operationId"), id},
                       {key("error"), QJsonObject{{key("message"), key("Test early failure")}}}});
            else {
                event(key("launcher://game-started"), {{key("operationId"), id}});
                event(key("launcher://error"),
                      {{key("operationId"), id},
                       {key("terminal"), false},
                       {key("error"),
                        QJsonObject{{key("message"), key("Test nonterminal warning")}}}});
            }
        } else if (method == key("stop_game"))
            event(key("launcher://game-exited"),
                  {{key("operationId"), params.value(key("operationId"))}});
        else if (method == key("get_profile") || method == key("select_build") ||
                 method == key("update_profile_memory")) {
            if (method == key("select_build"))
                active = params.value(key("buildId")).toString();
            if (method == key("update_profile_memory"))
                profile[key("memoryMb")] = params.value(key("memoryMb"));
            result = profile;
        } else if (method == key("list_builds")) {
            QJsonArray builds;
            const int count = qEnvironmentVariable("CK_TEST_MANY_BUILDS") == key("1") ? 12 : 2;
            for (int i = 0; i < count; ++i)
                builds.append(
                    QJsonObject{{key("id"), i == 0   ? key("fo")
                                            : i == 1 ? key("sodium")
                                                     : key("extra-") + QString::number(i)},
                                {key("name"), i == 0 ? key("Fabulously Optimized") : key("2.4.3")},
                                {key("gameVersion"), key("fabric-loader-0.16.14-1.21.1")},
                                {key("loaderVersion"), key("0.16.14")},
                                {key("loader"), key("fabric")},
                                {key("isActive"), active == (i == 0 ? key("fo") : key("sodium"))},
                                {key("iconUrl"), i == 0 ? fo : sodium}});
            result = builds;
        } else if (method == key("list_installed_content"))
            result = content;
        else if (method == key("set_installed_content_enabled")) {
            for (int i = 0; i < content.size(); ++i) {
                auto c = content[i].toObject();
                if (c.value(key("projectId")) == params.value(key("projectId"))) {
                    c[key("enabled")] = params.value(key("enabled"));
                    content[i] = c;
                }
            }
        } else if (method == key("list_accounts")) {
            QJsonArray accounts;
            if (loggedIn)
                for (int i = 1; i <= 2; ++i)
                    accounts.append(QJsonObject{
                        {key("id"), key("account-") + QString::number(i)},
                        {key("minecraftName"), i == 1 ? key("Player") : key("Alex")},
                        {key("minecraftUuid"), key("test-only")},
                        {key("isActive"), account == key("account-") + QString::number(i)},
                        {key("headUrl"), key("https://mc-heads.net/avatar/test-fixture/64")}});
            result = accounts;
        } else if (method == key("set_active_account"))
            account = params.value(key("accountId")).toString();
        else if (method == key("begin_microsoft_login"))
            loggedIn = true;
        else if (method == key("list_offline_skins"))
            result = skins;
        else if (method == key("minecraft_cosmetics"))
            result = QJsonObject{
                {key("skins"),
                 QJsonArray{QJsonObject{
                     {key("id"), key("current")},
                     {key("state"), key("ACTIVE")},
                     {key("variant"), key("CLASSIC")},
                     {key("url"), key("http://textures.minecraft.net/texture/test-fixture")}}}},
                {key("capes"), capes}};
        else if (method == key("set_offline_skin_favorite") ||
                 method == key("rename_offline_skin") || method == key("apply_minecraft_skin")) {
            for (int i = 0; i < skins.size(); ++i) {
                auto skin = skins[i].toObject();
                if (skin.value(key("id")) != params.value(key("skinId")))
                    continue;
                if (method == key("apply_minecraft_skin"))
                    currentSkin = skin.value(key("dataUrl")).toString();
                else if (method == key("rename_offline_skin"))
                    skin[key("name")] = params.value(key("name"));
                else
                    skin[key("isFavorite")] = params.value(key("isFavorite"));
                skins[i] = skin;
            }
        } else if (method == key("activate_minecraft_cape")) {
            for (int i = 0; i < capes.size(); ++i) {
                auto cape = capes[i].toObject();
                cape[key("state")] = cape.value(key("id")) == params.value(key("capeId"))
                                         ? key("ACTIVE")
                                         : key("INACTIVE");
                capes[i] = cape;
            }
        } else if (method == key("check_update")) {
            result = QJsonObject{{key("available"), false}};
        } else if (method == key("runtime_statuses")) {
            QJsonArray runtimes;
            for (int n : {8, 16, 17, 21, 25})
                runtimes.append(QJsonObject{
                    {key("requirement"), n},
                    {key("state"), n == 21 ? key("valid") : key("missing")},
                    {key("path"),
                     n == 21 ? key("C:/Program Files/Java/jdk-21/bin/java.exe") : QString()}});
            result = runtimes;
        } else if (method == key("memory_status"))
            result = QJsonObject{{key("minMemoryMb"), 1024},
                                 {key("maxMemoryMb"), 16384},
                                 {key("stepMemoryMb"), 512}};
        else if (method == key("list_game_versions"))
            result =
                QJsonArray{QJsonObject{{key("id"), key("1.21.1")}, {key("type"), key("release")}},
                           QJsonObject{{key("id"), key("1.20.1")}, {key("type"), key("release")}}};
        else if (method == key("install_modrinth_project") ||
                 method == key("install_modrinth_modpack")) {
            if (holdInstall) {
                heldInstall = request;
                continue;
            }
            result = QJsonObject{};
        } else if (method == key("test_hold_install")) {
            holdInstall = true;
        } else if (method == key("test_finish_install")) {
            holdInstall = false;
            if (!heldInstall.isEmpty()) {
                QJsonObject response{{key("id"), heldInstall.value(key("id"))}};
                if (params.contains(key("error")))
                    response[key("error")] =
                        QJsonObject{{key("message"), params.value(key("error"))}};
                else
                    response[key("result")] = QJsonObject{};
                std::cout << QJsonDocument(response).toJson(QJsonDocument::Compact).constData()
                          << std::endl;
                heldInstall = {};
            }
        } else if (method == key("test_wait")) {
            continue;
        } else if (method == key("test_fail_next")) {
            failNext[params.value(key("method")).toString()] =
                params.value(key("message")).toString();
        } else if (method == key("modrinth_project"))
            result = QJsonObject{
                {key("id"), params.value(key("projectId"))},
                {key("title"), key("Fabulously Optimized")},
                {key("description"), key("A fast, beautiful Minecraft experience.")},
                {key("icon_url"), fo},
                {key("bodyHtml"),
                 key("<div align='center'><h1>Performance</h1><p><b>Fast and "
                     "beautiful.</b></p></div>"
                     "<h2>Included</h2><ul><li>Performance improvements</li><li>Familiar "
                     "graphics</li><li>Easy installation</li></ul>"
                     "<p>Pok&eacute;mon &amp; friends</p><img "
                     "src='https://cdn.modrinth.com/banner.png'>"
                     "<p>Text after the banner</p><a href='https://modrinth.com'>Project "
                     "website</a>"
                     "<table><tr><th>Feature</th><th>Included</th></tr><tr><td>Optimization</"
                     "td><td>Yes</td></tr></table>")},
                {key("body"),
                 key("# Performance\n\n**Fast and beautiful.** Keep the features you love.\n\n"
                     "## Included\n\n- Performance improvements\n- Familiar graphics\n- Easy "
                     "installation\n\n"
                     "[Project website](https://modrinth.com)\n\n"
                     "| Feature | Included |\n|---|---|\n| Optimization | Yes |")}};
        else if (method == key("modrinth_project_versions"))
            result = QJsonArray{QJsonObject{{key("id"), key("compatible")},
                                            {key("version_number"), key("1.0")},
                                            {key("game_versions"), QJsonArray{key("1.21.1")}},
                                            {key("loaders"), QJsonArray{key("fabric")}}},
                                QJsonObject{{key("id"), key("wrong-version")},
                                            {key("version_number"), key("0.1")},
                                            {key("game_versions"), QJsonArray{key("1.20.1")}},
                                            {key("loaders"), QJsonArray{key("forge")}}}};
        else if (method == key("search_modrinth")) {
            QJsonArray hits;
            for (int i = 0; i < 4; ++i)
                hits.append(QJsonObject{
                    {key("project_id"),
                     i == 0 ? key("Fabulously Optimized") : key("project-") + QString::number(i)},
                    {key("title"), i == 0   ? key("Fabulously Optimized")
                                   : i == 1 ? key("Sodium Plus")
                                   : i == 2 ? key("Vanilla Perfected")
                                            : key("OptiFabric")},
                    {key("author"), i == 0 ? key("robotkoer") : key("Community")},
                    {key("description"), key("Beautiful graphics, speedy performance and familiar "
                                             "features in a simple package.")},
                    {key("project_type"), params.value(key("projectType"))},
                    {key("icon_url"), i == 0 ? fo : sodium},
                    {key("categories"),
                     QJsonArray{key("fabric"), key("lightweight"), key("optimization")}}});
            result = QJsonObject{{key("hits"), hits}, {key("total_hits"), 42}};
        } else if (method == key("list_build_files"))
            result = QJsonArray{QJsonObject{{key("name"), key("mods")},
                                            {key("kind"), key("directory")},
                                            {key("relativePath"), key("mods")}},
                                QJsonObject{{key("name"), key("options.txt")},
                                            {key("kind"), key("file")},
                                            {key("relativePath"), key("options.txt")},
                                            {key("size"), 2080}}};
        else if (method == key("list_build_worlds"))
            result = QJsonArray{QJsonObject{{key("name"), QString::fromUtf8("Мир с друзьями")},
                                            {key("relativePath"), key("saves/world")},
                                            {key("size"), 16000000}}};
        else if (method == key("list_build_logs"))
            result = QJsonArray{QJsonObject{{key("name"), key("latest.log")},
                                            {key("relativePath"), key("logs/latest.log")}}};
        else if (method == key("read_latest_game_log") || method == key("read_build_log"))
            result =
                key("[Render thread/INFO] Test fixture: Minecraft ready.\nNo game was launched.");
        else if (method == key("test_state"))
            result = QJsonObject{{key("requests"), requests},
                                 {key("content"), content},
                                 {key("activeBuild"), active},
                                 {key("profile"), profile},
                                 {key("account"), account}};
        else if (method == key("create_build") || method == key("cancel_microsoft_login") ||
                 method == key("cancel_content_operation")) {
        } else
            known = false;
        QJsonObject reply{{key("id"), request.value(key("id"))}};
        // Optional recorded public responses for release QA; this executable is never shipped.
        const auto recorded = qEnvironmentVariable("CK_QA_PROJECT_DIR");
        if (!recorded.isEmpty() &&
            (method == key("modrinth_project") || method == key("load_public_image"))) {
            QFile file(recorded + (method == key("modrinth_project") ? key("/cobblemon.json")
                                                                     : key("/public-images.json")));
            if (file.open(QIODevice::ReadOnly)) {
                const auto fixture = QJsonDocument::fromJson(file.read(32 * 1024 * 1024)).object();
                if (method == key("modrinth_project"))
                    result = fixture;
                else if (fixture.contains(params.value(key("url")).toString()))
                    result = fixture.value(params.value(key("url")).toString());
            }
        }
        const auto projectId = params.value(key("projectId")).toString();
        QString testError = failNext.take(method);
        if (projectId.startsWith(key("project-test"))) {
            if (method == key("modrinth_project")) {
                auto project = result.toObject();
                project[key("project_type")] =
                    projectId == key("project-test-mod") ? key("mod") : key("modpack");
                result = project;
                if (projectId == key("project-test-metadata-error"))
                    testError = key("Test project unavailable");
            } else if (method == key("modrinth_project_versions")) {
                QJsonArray versions;
                for (int i = 0; i < 3; ++i)
                    versions.append(QJsonObject{
                        {key("id"), i == 0   ? key("pv-new")
                                    : i == 1 ? key("pv-old")
                                             : key("pv-compatible-old")},
                        {key("version_number"),
                         i == 2 ? QString(150, QChar('W')) : key("2.") + QString::number(3 - i)},
                        {key("name"), key("Test release ") + QString::number(i)},
                        {key("version_type"), i == 0   ? key("release")
                                              : i == 1 ? key("beta")
                                                       : key("alpha")},
                        {key("date_published"),
                         key("2026-09-0") + QString::number(3 - i) + key("T10:00:00Z")},
                        {key("game_versions"), QJsonArray{i == 1 ? key("1.20.1") : key("1.21.1")}},
                        {key("loaders"), QJsonArray{i == 1 ? key("forge") : key("fabric")}}});
                result = projectId == key("project-test-empty") ? QJsonArray{} : versions;
                if (projectId == key("project-test-error") && versionErrorAttempts++ == 0)
                    testError = key("Test versions unavailable");
            }
        }
        if (known)
            reply[key("result")] = result;
        else
            reply[key("error")] =
                QJsonObject{{key("message"), key("Unhandled test method: ") + method}};
        if (!testError.isEmpty()) {
            reply.remove(key("result"));
            reply[key("error")] = QJsonObject{{key("message"), testError}};
        }
        std::cout << QJsonDocument(reply).toJson(QJsonDocument::Compact).constData() << std::endl;
    }
}
