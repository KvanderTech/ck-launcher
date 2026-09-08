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
                QCOMPARE(window->findChild<QPushButton *>(s("skin-card-current"))->height(), 280);
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
        auto *title = namedButton(s("2.4.3"));
        QVERIFY(title);
        QTest::mouseClick(title, Qt::LeftButton);
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
        bool reviewed = false;
        QTimer::singleShot(250, this, [&] {
            auto *dialog = window->findChild<QInputDialog *>();
            if (dialog) {
                auto *choices = dialog->findChild<QComboBox *>();
                reviewed =
                    choices && choices->count() == 1 && choices->itemText(0).contains(s("1.21.1"));
                dialog->reject();
            }
        });
        QTest::mouseClick(install, Qt::LeftButton);
        QTRY_VERIFY_WITH_TIMEOUT(reviewed, 1500);
    }
    void skinPreviewAndAccountPopup() {
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
