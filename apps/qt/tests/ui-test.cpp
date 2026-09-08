#include "emote.h"
#include "window.h"
#include <QtTest>
class UiTest final : public QObject {
    Q_OBJECT
    QTemporaryDir temporary;
    Backend *backend = nullptr;
    LauncherWindow *window = nullptr;
    QString output;
    void snapshot(const QString &name) {
        QTest::qWait(250);
        QVERIFY(window->grab().save(output + s("/") + name + s(".png")));
    }
    QJsonObject state() {
        auto result = std::make_shared<QJsonObject>();
        auto done = std::make_shared<bool>(false);
        backend->request(s("test_state"), {},
                         [result, done](const QJsonValue &v, const QJsonObject &e) {
                             if (e.isEmpty())
                                 *result = v.toObject();
                             *done = true;
                         });
        for (int i = 0; i < 50 && !*done; ++i)
            QTest::qWait(20);
        return *result;
    }
    QPushButton *namedButton(const QString &text, QWidget *root = nullptr) {
        for (auto *b : (root ? root : window)->findChildren<QPushButton *>())
            if (b->text() == text && b->isVisible())
                return b;
        return nullptr;
    }
  private slots:
    void initTestCase() {
        QVERIFY(temporary.isValid());
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, temporary.path());
        QCoreApplication::setOrganizationName(s("CKLauncherTests"));
        QCoreApplication::setApplicationName(s("Interface"));
        QSettings().setValue(s("motion"), false);
        for (const auto &font :
             {s("segoeui.ttf"), s("segoeuib.ttf"), s("seguisb.ttf"), s("seguibl.ttf")})
            QFontDatabase::addApplicationFont(qEnvironmentVariable("WINDIR") + s("/Fonts/") + font);
        qApp->setStyle(s("Fusion"));
        QFile style(s(":/assets/theme.qss"));
        QVERIFY(style.open(QIODevice::ReadOnly));
        qApp->setStyleSheet(QString::fromUtf8(style.readAll()));
        output = qEnvironmentVariable("CK_UI_SCREENSHOTS");
        if (output.isEmpty())
            output = QDir::currentPath() + s("/ui-screenshots");
        QVERIFY(QDir().mkpath(output));
        backend = new Backend(this);
        window = new LauncherWindow(backend);
        window->show();
        auto environment = QProcessEnvironment::systemEnvironment();
        environment.insert(s("APPDATA"), temporary.path());
        backend->start(QCoreApplication::applicationDirPath() + s("/ui-fixture.exe"), environment);
        QTRY_VERIFY_WITH_TIMEOUT(window->findChild<QPushButton *>(s("playButton"))->isEnabled(),
                                 10000);
        QTRY_VERIFY_WITH_TIMEOUT(window->findChild<QFrame *>(s("build-card-fo")), 10000);
    }
    void originalLayoutAndGallery() {
        window->resize(1280, 720);
        window->showPage(s("home"));
        snapshot(s("home"));
        QCOMPARE(window->findChild<QWidget *>(s("sidebar"))->width(), 72);
        QCOMPARE(window->findChild<QPushButton *>(s("playButton"))->size(), QSize(205, 56));
        auto *sidebarScroll = window->findChild<QScrollArea *>(s("sidebar-builds-scroll"));
        auto *secondShortcut = window->findChild<QPushButton *>(s("build-shortcut-sodium"));
        QVERIFY(sidebarScroll->viewport()->rect().contains(QRect(
            secondShortcut->mapTo(sidebarScroll->viewport(), QPoint()), secondShortcut->size())));
        for (const auto &page :
             {s("library"), s("catalog"), s("skins"), s("settings"), s("details"), s("logs")}) {
            window->showPage(page);
            QTest::qWait(150);
            snapshot(page);
            if (page == s("library")) {
                auto *one = window->findChild<QFrame *>(s("build-card-fo"));
                auto *two = window->findChild<QFrame *>(s("build-card-sodium"));
                QVERIFY(one->isVisible());
                QVERIFY(two->isVisible());
                QCOMPARE(one->y(), two->y());
                QVERIFY(one->width() > 490);
            }
            if (page == s("skins")) {
                QTRY_VERIFY(window->findChild<QPushButton *>(s("skin-card-current")));
                QCOMPARE(window->findChild<QPushButton *>(s("skin-card-current"))->height(), 296);
                QCOMPARE(window->findChild<QPushButton *>(s("skin-card-current"))->width(), 174);
            }
        }
        window->showPage(s("library"));
        window->resize(980, 620);
        QCOMPARE(window->size(), QSize(980, 620));
        QTest::qWait(250);
        auto *one = window->findChild<QFrame *>(s("build-card-fo"));
        auto *two = window->findChild<QFrame *>(s("build-card-sodium"));
        QVERIFY(two->y() > one->y());
        snapshot(s("library-small"));
        for (const auto &page : {s("catalog"), s("skins"), s("settings"), s("details")}) {
            window->showPage(page);
            QTest::qWait(250);
            auto *scroll = window->findChild<QScrollArea *>();
            QVERIFY(scroll);
            for (auto *area : window->findChildren<QScrollArea *>())
                if (area->isVisible())
                    QVERIFY2(area->widget()->width() <= area->viewport()->width(),
                             "Page overflows horizontally at minimum window size");
            snapshot(page + s("-small"));
        }
        window->resize(1280, 720);
    }
    void selectsBuildAndTogglesContent() {
        window->showPage(s("library"));
        auto *card = window->findChild<QFrame *>(s("build-card-sodium"));
        QVERIFY(card);
        // The lower empty part of the panel, not its name or Play button.
        QTest::mouseClick(card, Qt::LeftButton, Qt::NoModifier,
                          QPoint(card->width() / 2, card->height() - 8));
        QTRY_COMPARE(state().value(s("activeBuild")).toString(), s("sodium"));
        auto *tabs = window->findChild<QTabBar *>(s("buildTabs"));
        tabs->setCurrentIndex(0);
        QTest::qWait(100);
        auto *toggle = namedButton(QString::fromUtf8("Включён"));
        for (auto *b : window->findChildren<QPushButton *>())
            if (b->isVisible() && b->isEnabled() && b->text() == QString::fromUtf8("Включён")) {
                toggle = b;
                break;
            }
        QVERIFY(toggle && toggle->isEnabled());
        QTest::mouseClick(toggle, Qt::LeftButton);
        QTRY_VERIFY([&] {
            for (const auto &item : state().value(s("content")).toArray())
                if (!item.toObject().value(s("enabled")).toBool())
                    return true;
            return false;
        }());
        QVERIFY(!window->findChild<QWidget *>(s("project-page"))->isVisible());
        QTRY_VERIFY(window->findChild<QFrame *>(s("content-card-SodiumTranslations")));
        auto *content = window->findChild<QFrame *>(s("content-card-SodiumTranslations"));
        QTest::mouseClick(content, Qt::LeftButton, Qt::NoModifier,
                          QPoint(content->width() / 2, content->height() - 6));
        QTRY_VERIFY(window->findChild<QWidget *>(s("project-page"))->isVisible());
        QTRY_VERIFY([&] {
            for (const auto &entry : state().value(s("requests")).toArray()) {
                const auto request = entry.toObject();
                if (value(request, "method") == s("modrinth_project") &&
                    value(request.value(s("params")).toObject(), "projectId") ==
                        s("SodiumTranslations"))
                    return true;
            }
            return false;
        }());
        QTest::mouseClick(window->findChild<QPushButton *>(s("project-back")), Qt::LeftButton);
        QTRY_VERIFY(!window->findChild<QWidget *>(s("project-page"))->isVisible());
    }
    void filtersCatalogAndPersistsMemory() {
        window->showPage(s("catalog"));
        auto *search = window->findChild<QLineEdit *>(s("catalog-search"));
        QVERIFY(search);
        search->setText(s("sodium"));
        QTest::keyClick(search, Qt::Key_Return);
        QTRY_VERIFY([&] {
            for (const auto &item : state().value(s("requests")).toArray()) {
                auto request = item.toObject();
                if (value(request, "method") == s("search_modrinth") &&
                    value(request.value(s("params")).toObject(), "query") == s("sodium"))
                    return true;
            }
            return false;
        }());
        window->showPage(s("settings"));
        auto *memory = window->findChild<QSpinBox *>(s("memory"));
        memory->setValue(6144);
        auto *save = window->findChild<QPushButton *>(s("save-memory"));
        QVERIFY(save->isVisible());
        QTest::mouseClick(save, Qt::LeftButton);
        QTRY_COMPARE(state().value(s("profile")).toObject().value(s("memoryMb")).toInt(), 6144);
    }
    void installedVersionsMatchFabricBaseVersion() {
        window->showPage(s("catalog"));
        auto *tabs = window->findChild<QTabBar *>(s("catalog-tabs"));
        tabs->setCurrentIndex(1);
        QTest::qWait(100);
        auto *install = namedButton(QString::fromUtf8("+ Установить"));
        QVERIFY(install);
        QTest::mouseClick(install, Qt::LeftButton);
        auto *choices = window->findChild<QComboBox *>(s("project-version"));
        QTRY_COMPARE_WITH_TIMEOUT(choices->count(), 1, 1500);
        QCOMPARE(choices->currentData().toString(), s("compatible"));
        QVERIFY(window->findChild<QTextBrowser *>(s("project-description"))->isVisible());
        QTRY_VERIFY(window->findChild<QTextBrowser *>(s("project-description"))
                        ->toPlainText()
                        .contains(s("Performance")));
        window->resize(1280, 720);
        snapshot(s("project-details"));
        auto *games = window->findChild<QComboBox *>(s("project-game"));
        games->setCurrentIndex(games->findData(s("1.20.1")));
        QCOMPARE(choices->count(), 0);
        QVERIFY(!window->findChild<QPushButton *>(s("project-install"))->isEnabled());
        games->setCurrentIndex(games->findData(s("1.21.1")));
        QCOMPARE(choices->count(), 1);
        QTest::mouseClick(window->findChild<QPushButton *>(s("project-install")), Qt::LeftButton);
        QTRY_VERIFY([&] {
            for (const auto &entry : state().value(s("requests")).toArray()) {
                const auto request = entry.toObject();
                if (value(request, "method") == s("install_modrinth_project"))
                    return value(request.value(s("params")).toObject(), "versionId") ==
                           s("compatible");
            }
            return false;
        }());
    }
    void soundSettingPersists() {
        window->showPage(s("settings"));
        auto *sounds = window->findChild<QCheckBox *>(s("sounds-setting"));
        QVERIFY(sounds);
        sounds->setChecked(false);
        QVERIFY(!AudioFeedback::isEnabled());
        QVERIFY(!QSettings().value(s("sounds"), true).toBool());
        sounds->setChecked(true);
        QVERIFY(AudioFeedback::isEnabled());
        sounds->setChecked(false);
    }
    void installationFailureAllowsRetryFromVersionRow() {
        window->showPage(s("catalog"));
        window->findChild<QTabBar *>(s("catalog-tabs"))->setCurrentIndex(0);
        QTest::qWait(100);
        auto *open = namedButton(QString::fromUtf8("+ Установить"));
        QVERIFY(open);
        QTest::mouseClick(open, Qt::LeftButton);
        auto *tabs = window->findChild<QTabBar *>(s("project-tabs"));
        QVERIFY(tabs);
        tabs->setCurrentIndex(1);
        auto row = [&] {
            return window->findChild<QPushButton *>(s("project-version-install-compatible"));
        };
        QTRY_VERIFY(row() && row()->isEnabled());
        backend->request(s("test_hold_install"));
        state(); // Flush the service queue before clicking.
        QTest::mouseClick(row(), Qt::LeftButton);
        QTRY_VERIFY(!row()->isEnabled());
        QVERIFY(!window->findChild<QPushButton *>(s("project-install"))->isEnabled());
        QVERIFY(!window->findChild<QComboBox *>(s("project-game"))->isEnabled());
        snapshot(s("project-installing"));
        const QString error =
            QString::fromUtf8("Проверка: соединение прервано. Повторите установку.");
        backend->request(s("test_finish_install"), {{s("error"), error}});
        QTRY_VERIFY(row()->isEnabled());
        QCOMPARE(window->findChild<QLabel *>(s("project-status"))->text(), error);
        snapshot(s("project-install-error"));
        QTest::mouseClick(row(), Qt::LeftButton);
        QTRY_VERIFY(!window->findChild<QWidget *>(s("project-page"))->isVisible());
        QVERIFY(window->findChild<QPushButton *>(s("playButton"))->isEnabled());
    }
    void networkErrorsStopLoadingAndAllowRetry() {
        const auto failure = QString::fromUtf8("Проверка: сеть недоступна");
        backend->request(s("test_fail_next"),
                         {{s("method"), s("check_update")}, {s("message"), failure}});
        state();
        window->showPage(s("settings"));
        auto *check = window->findChild<QPushButton *>(s("check-update"));
        auto *status = window->findChild<QLabel *>(s("update-status"));
        QVERIFY(check && status);
        QTest::mouseClick(check, Qt::LeftButton);
        QTRY_COMPARE(status->text(), failure);
        QVERIFY(check->isEnabled());
        QTest::mouseClick(check, Qt::LeftButton);
        QTRY_COMPARE(status->text(), QString::fromUtf8("Установлена актуальная версия"));
        QVERIFY(check->isEnabled());
        window->showPage(s("catalog"));
        backend->request(s("test_fail_next"),
                         {{s("method"), s("search_modrinth")}, {s("message"), failure}});
        state();
        auto *search = window->findChild<QLineEdit *>(s("catalog-search"));
        QTest::keyClick(search, Qt::Key_Return);
        const auto hasLabel = [this](const QString &text) {
            for (auto *label : window->findChildren<QLabel *>())
                if (label->isVisible() && label->text() == text)
                    return true;
            return false;
        };
        QTRY_VERIFY(hasLabel(QString::fromUtf8("Не удалось загрузить проекты. Повторите поиск.")));
        QVERIFY(!hasLabel(QString::fromUtf8("Загружаем проекты…")));
        QTest::keyClick(search, Qt::Key_Return);
        QTRY_VERIFY(!hasLabel(QString::fromUtf8("Не удалось загрузить проекты. Повторите поиск.")));
    }
    void manyBuildsDoNotPushWindowOffScreen() {
        Backend isolated;
        LauncherWindow gallery(&isolated);
        gallery.resize(980, 620);
        gallery.show();
        auto environment = QProcessEnvironment::systemEnvironment();
        environment.insert(s("CK_TEST_MANY_BUILDS"), s("1"));
        environment.insert(s("APPDATA"), temporary.path());
        isolated.start(QCoreApplication::applicationDirPath() + s("/ui-fixture.exe"), environment);
        QTRY_VERIFY(gallery.findChild<QPushButton *>(s("build-shortcut-extra-11")));
        QTest::qWait(200);
        QCOMPARE(gallery.size(), QSize(980, 620));
        auto *scroll = gallery.findChild<QScrollArea *>(s("sidebar-builds-scroll"));
        QVERIFY(scroll->verticalScrollBar()->maximum() > 0);
        auto *account = gallery.findChild<QPushButton *>(s("accountButton"));
        QVERIFY(
            gallery.rect().contains(QRect(account->mapTo(&gallery, QPoint()), account->size())));
        QVERIFY(gallery.grab().save(output + s("/home-many-builds-small.png")));
        isolated.shutdown();
    }
    void cancellationHasReservedQueueCapacity() {
        Backend isolated;
        QSignalSpy ready(&isolated, &Backend::ready);
        isolated.start(QCoreApplication::applicationDirPath() + s("/ui-fixture.exe"),
                       QProcessEnvironment::systemEnvironment());
        QTRY_COMPARE(ready.count(), 1);
        for (int i = 0; i < 64; ++i)
            isolated.request(s("test_wait"));
        auto controlFinished = std::make_shared<bool>(false);
        auto rejected = std::make_shared<bool>(false);
        isolated.request(
            s("get_profile"), {},
            [rejected](const QJsonValue &, const QJsonObject &e) { *rejected = !e.isEmpty(); });
        isolated.request(s("cancel_content_operation"), {},
                         [controlFinished](const QJsonValue &, const QJsonObject &e) {
                             *controlFinished = e.isEmpty();
                         });
        QTRY_VERIFY(*rejected);
        QTRY_VERIFY(*controlFinished);
        isolated.shutdown();
    }
    void iconCornersAndArtworkAreIntact() {
        QImage source(60, 30, QImage::Format_ARGB32);
        source.fill(QColor(111, 198, 83));
        const auto icon = roundedIcon(source);
        const auto pixels = icon.pixmap(36, 36).toImage();
        QVERIFY(pixels.pixelColor(0, 0).alpha() < 5);
        QCOMPARE(pixels.pixelColor(pixels.width() / 2, pixels.height() / 2).green(), 198);
        QVERIFY(!QImage(s(":/assets/home-render.png")).isNull());
        window->showPage(s("home"));
        window->resize(980, 620);
        snapshot(s("home-small"));
        const auto art = window->findChild<QWidget *>(s("home-artwork"))->grab().toImage();
        int colourful = 0;
        for (int y = 0; y < art.height(); y += 4)
            for (int x = art.width() / 2; x < art.width(); x += 4) {
                const auto color = art.pixelColor(x, y);
                if (color.red() > 130 && color.red() > color.blue() + 20)
                    ++colourful;
            }
        QVERIFY2(colourful > 50, "Home artwork is missing or clipped off-screen");
        window->resize(1280, 720);
    }
    void exactKeyframesAndJointBends() {
        const auto yes = EmoteClip::load(s(":/assets/emotes/yes.json"));
        const auto wave = EmoteClip::load(s(":/assets/emotes/wave.json"));
        QVERIFY(yes.valid());
        QVERIFY(wave.valid());
        QVERIFY(std::abs(wave.sample(s("rightArm"), 20, {-5, 2, 0}).bend) > .1);
        const QVector3D hand(0, -10, 0), shoulder(0, 2, 0);
        const auto bent =
            EmoteClip::bendVertex(hand, {0, -4, 0}, 6, float(-3.141592653589793 / 2), 0);
        QVERIFY(std::abs(bent.z()) > 5.9);
        QCOMPARE(EmoteClip::bendVertex(shoulder, {0, -4, 0}, 6, 1, 0), shoulder);
        QCOMPARE(EmoteClip::bendVertex(hand, {0, -4, 0}, 6, 0, 0), hand);
        const auto rest = yes.sample(s("rightArm"), 15, {-5, 2, 0});
        QCOMPARE(rest.position, QVector3D(-5, 2, 0));
    }
    void skinPreviewAndAccountPopup() {
        window->resize(1280, 720);
        window->showPage(s("skins"));
        QTest::qWait(150);
        auto *preview = static_cast<SkinView *>(window->findChild<QWidget *>(s("skin-preview")));
        QVERIFY(preview);
        const auto idle = preview->grab().toImage();
        preview->setAnimated(true);
        preview->setPoseTime(7.0);
        QTest::qWait(80);
        const auto pose = preview->grab().toImage();
        QVERIFY(idle != pose);
        snapshot(s("skin-animation"));
        preview->setPoseTime(11.5);
        snapshot(s("skin-bent-elbow"));
        const auto beforeDrag = preview->grab().toImage();
        QMouseEvent press(QEvent::MouseButtonPress, QPointF(100, 120), Qt::LeftButton,
                          Qt::LeftButton, Qt::NoModifier);
        QMouseEvent move(QEvent::MouseMove, QPointF(155, 120), Qt::NoButton, Qt::LeftButton,
                         Qt::NoModifier);
        QApplication::sendEvent(preview, &press);
        QApplication::sendEvent(preview, &move);
        QVERIFY(preview->grab().toImage() != beforeDrag);
        preview->setAnimated(false);
        auto *card = window->findChild<QPushButton *>(s("skin-card-skin-1"));
        QVERIFY(card);
        QTest::mouseClick(card, Qt::LeftButton, Qt::NoModifier,
                          QPoint(card->width() / 2, card->height() / 2));
        QTRY_VERIFY(window->findChild<QWidget *>(s("skin-apply-row"))->isVisible());
        QTest::mouseClick(window->findChild<QPushButton *>(s("accountButton")), Qt::LeftButton);
        QTRY_VERIFY(window->findChild<QFrame *>(s("account-popup")));
        auto *popup = window->findChild<QFrame *>(s("account-popup"));
        QVERIFY(popup->grab().save(output + s("/account-popup.png")));
        popup->close();
    }
    void buttonsAnimateWithoutMoving() {
        window->showPage(s("home"));
        auto *button = window->findChild<QPushButton *>(s("playButton"));
        qApp->setProperty("reduceMotion", false);
        const auto box = button->geometry();
        QEvent leave(QEvent::Leave);
        QApplication::sendEvent(button, &leave);
        QTest::qWait(220);
        const auto before = button->grab().toImage();
        QEvent enter(QEvent::Enter);
        QApplication::sendEvent(button, &enter);
        QTest::qWait(220);
        QVERIFY(before != button->grab().toImage());
        QCOMPARE(box, button->geometry());
        qApp->setProperty("reduceMotion", true);
    }
    void trustedImagesUseHttps() {
        bool fetched = false;
        for (const auto &request : state().value(s("requests")).toArray())
            if (value(request.toObject(), "method") == s("load_public_image")) {
                fetched = true;
                QVERIFY(value(request.toObject().value(s("params")).toObject(), "url")
                            .startsWith(s("https://")));
            }
        QVERIFY(fetched);

        QVERIFY(QImageReader::supportedImageFormats().contains("webp"));
        QVERIFY(QImageReader::supportedImageFormats().contains("jpeg"));
        QCOMPARE(ImagePool::remoteUrl(s("http://textures.minecraft.net/texture/abc")).scheme(),
                 s("https"));
        QVERIFY(ImagePool::remoteUrl(s("https://cdn.modrinth.com/data/icon.png")).isValid());
        for (const auto &url :
             {s("https://cdn.modrinth.com.attacker.test/icon.png"), s("file:///C:/secret.png"),
              s("http://127.0.0.1/icon.png"), s("https://user:secret@cdn.modrinth.com/icon.png"),
              s("https://textures.minecraft.net:8080/texture/a")})
            QVERIFY(ImagePool::remoteUrl(url).isEmpty());
    }
    void earlyFailureAndNonterminalWarningPreserveLaunchState() {
        window->showPage(s("home"));
        auto *play = window->findChild<QPushButton *>(s("playButton"));
        QVERIFY(play->isEnabled());
        QTest::mouseClick(play, Qt::LeftButton);
        QTRY_VERIFY([&] {
            for (const auto &r : state().value(s("requests")).toArray())
                if (value(r.toObject(), "method") == s("launch_or_install"))
                    return true;
            return false;
        }());
        QTRY_VERIFY(play->isEnabled());
        QCOMPARE(play->text(), QString::fromUtf8("Играть"));
        QTest::mouseClick(play, Qt::LeftButton);
        QTRY_COMPARE(play->text(), QString::fromUtf8("Игра запущена"));
        QVERIFY(!play->isEnabled());
        bool stopped = false;
        backend->request(s("stop_game"), {{s("operationId"), s("launch-2")}},
                         [&](const QJsonValue &, const QJsonObject &) { stopped = true; });
        QTRY_VERIFY(stopped);
        QTRY_VERIFY(play->isEnabled());
    }
    void signedOutGateAndReturnToSkins() {
        Backend isolated;
        LauncherWindow login(&isolated);
        login.resize(1280, 720);
        login.show();
        login.showPage(s("skins"));
        auto environment = QProcessEnvironment::systemEnvironment();
        environment.insert(s("CK_TEST_NO_ACCOUNT"), s("1"));
        environment.insert(s("APPDATA"), temporary.path());
        isolated.start(QCoreApplication::applicationDirPath() + s("/ui-fixture.exe"), environment);
        QTRY_VERIFY(login.findChild<QPushButton *>(s("playButton"))->isEnabled());
        auto *enter = login.findChild<QPushButton *>(s("microsoft-login"));
        QVERIFY(enter->isVisible());
        QCOMPARE(login.findChild<QFrame *>(s("login-card"))->width(), 760);
        auto *title = login.findChild<QLabel *>(s("login-heading"));
        QVERIFY(title->width() >= title->fontMetrics().horizontalAdvance(title->text()));
        QVERIFY(login.grab().save(output + s("/login.png")));
        QTest::mouseClick(enter, Qt::LeftButton);
        QTRY_VERIFY(login.findChild<QPushButton *>(s("skin-card-current")));
        QVERIFY(!enter->isVisible());
        isolated.shutdown();
    }
    void cleanupTestCase() {
        backend->shutdown();
        delete window;
        window = nullptr;
    }
};
int main(int argc, char **argv) {
#if QT_VERSION < QT_VERSION_CHECK(6, 0, 0)
    QCoreApplication::setAttribute(Qt::AA_EnableHighDpiScaling);
    QCoreApplication::setAttribute(Qt::AA_UseHighDpiPixmaps);
#endif
    QApplication app(argc, argv);
    UiTest test;
    return QTest::qExec(&test, argc, argv);
}
#include "ui-test.moc"
