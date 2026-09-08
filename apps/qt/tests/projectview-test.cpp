#include "projectview.h"
#include <QtTest>

class ProjectViewTest final : public QObject {
    Q_OBJECT
    QTemporaryDir temporary;
    Backend *backend = nullptr;
    ImagePool *images = nullptr;
    ProjectView *view = nullptr;
    QString output;

    QJsonArray builds() const {
        return {QJsonObject{{s("id"), s("fabric-build")},
                            {s("name"), s("Fabric build")},
                            {s("gameVersion"), s("fabric-loader-0.16.14-1.21.1")},
                            {s("loader"), s("fabric")},
                            {s("loaderVersion"), s("0.16.14")}},
                QJsonObject{{s("id"), s("forge-build")},
                            {s("name"), s("Forge build")},
                            {s("gameVersion"), s("forge-loader-47.4.10-1.20.1")},
                            {s("loader"), s("forge")},
                            {s("loaderVersion"), s("47.4.10")}},
                QJsonObject{{s("id"), s("older-build")},
                            {s("name"), s("Older build")},
                            {s("gameVersion"), s("1.19.4")},
                            {s("loader"), s("vanilla")}}};
    }

    void open(const QString &id = s("project-test"), const QString &type = s("modpack")) {
        view->open({{s("id"), id}, {s("project_type"), type}, {s("title"), s("Test project")}},
                   builds(), s("fabric-build"));
    }

    QComboBox *choices() const {
        return view->findChild<QComboBox *>(s("project-version"));
    }
    QTableWidget *table() const {
        return view->findChild<QTableWidget *>(s("project-versions-table"));
    }
    QPushButton *install() const {
        return view->findChild<QPushButton *>(s("project-install"));
    }
    QPushButton *rowButton(const QString &id) const {
        // Removed cell widgets can remain QObject children until DeferredDelete.
        // Resolve the live model row, not a retired button with the same object name.
        const auto index = row(id);
        auto *cell = index < 0 ? nullptr : table()->cellWidget(index, 1);
        return cell ? cell->findChild<QPushButton *>(s("project-version-install-") + id) : nullptr;
    }
    QLabel *status() const {
        return view->findChild<QLabel *>(s("project-status"));
    }
    void versionsTab() {
        view->findChild<QTabBar *>(s("project-tabs"))->setCurrentIndex(1);
    }
    int row(const QString &id) const {
        for (int i = 0; i < table()->rowCount(); ++i)
            if (table()->item(i, 0)->data(Qt::UserRole).toString() == id)
                return i;
        return -1;
    }
    void snapshot(const QString &name) {
        QTest::qWait(80);
        QVERIFY(view->grab().save(output + s("/") + name + s(".png")));
    }

  private slots:
    void initTestCase() {
        QVERIFY(temporary.isValid());
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, temporary.path());
        QCoreApplication::setOrganizationName(s("CKLauncherTests"));
        QCoreApplication::setApplicationName(s("Projects"));
        for (const auto &font :
             {s("segoeui.ttf"), s("segoeuib.ttf"), s("seguisb.ttf"), s("seguibl.ttf")})
            QFontDatabase::addApplicationFont(qEnvironmentVariable("WINDIR") + s("/Fonts/") + font);
        qApp->setProperty("reduceMotion", true);
        qApp->setStyle(s("Fusion"));
        QFile theme(s(":/assets/theme.qss"));
        QVERIFY(theme.open(QIODevice::ReadOnly));
        qApp->setStyleSheet(QString::fromUtf8(theme.readAll()));
        output = qEnvironmentVariable("CK_UI_SCREENSHOTS");
        if (output.isEmpty())
            output = QDir::currentPath() + s("/ui-screenshots");
        QVERIFY(QDir().mkpath(output));
        backend = new Backend(this);
        images = new ImagePool(backend, this);
        QSignalSpy ready(backend, &Backend::ready);
        auto environment = QProcessEnvironment::systemEnvironment();
        environment.insert(s("APPDATA"), temporary.path());
        backend->start(QCoreApplication::applicationDirPath() + s("/ui-fixture.exe"), environment);
        QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 10000);
    }

    void init() {
        view = new ProjectView(backend, images);
        view->setStyleSheet(s("QWidget#project-page {background:#071321;}"));
        view->resize(1180, 650);
        view->show();
    }

    void cleanup() {
        delete view;
        view = nullptr;
    }

    void openingProjectsReleasesDescriptionDocumentsSafely() {
        auto *description = view->findChild<QTextBrowser *>(s("project-description"));
        QVERIFY(description);
        QPointer<QTextDocument> initial = description->document();
        QVERIFY(initial);
        open();
        // The browser owns and destroys its initial document synchronously.
        // Accessing the old raw pointer after setDocument() is a use-after-free.
        QVERIFY(initial.isNull());
        QTRY_VERIFY(install()->isEnabled());
        for (int i = 0; i < 4; ++i) {
            QPointer<QTextDocument> previous = description->document();
            QVERIFY(previous);
            QCOMPARE(previous->parent(), description);
            open();
            QVERIFY(description->document() != previous.data());
            QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
            QVERIFY(previous.isNull());
            QTRY_COMPARE(choices()->count(), 3);
            QTRY_VERIFY(install()->isEnabled());
        }
        QCOMPARE(description->findChildren<QTextDocument *>(QString(), Qt::FindDirectChildrenOnly)
                     .count(),
                 1);
    }

    void rowInstallsItsExactVersion() {
        open();
        QTRY_COMPARE(choices()->count(), 3);
        QTRY_VERIFY(install()->isEnabled());
        versionsTab();
        QCOMPARE(table()->columnCount(), 2);
        QCOMPARE(choices()->currentData().toString(), s("pv-new"));
        QSignalSpy requested(view, &ProjectView::installRequested);
        auto *older = rowButton(s("pv-old"));
        QVERIFY(older);
        QVERIFY(older->isVisible());
        QTest::mouseClick(older, Qt::LeftButton);
        QCOMPARE(requested.count(), 1);
        const auto request = requested.takeFirst();
        QCOMPARE(request[0].toString(), s("project-test"));
        QCOMPARE(request[1].toString(), s("pv-old"));
        QVERIFY(request[2].toString().isEmpty());
        QVERIFY(request[3].toBool());
        QCOMPARE(choices()->currentData().toString(), s("pv-old"));
        QVERIFY(!install()->isEnabled());
        QVERIFY(!older->isEnabled());
        QVERIFY(!rowButton(s("pv-new"))->isEnabled());
        QVERIFY(!choices()->isEnabled());
        QTest::mouseClick(older, Qt::LeftButton);
        QCOMPARE(requested.count(), 0);
    }

    void filtersAndTableKeepSelectionById() {
        open();
        QTRY_COMPARE(choices()->count(), 3);
        QTRY_VERIFY(install()->isEnabled());
        versionsTab();
        choices()->setCurrentIndex(choices()->findData(s("pv-compatible-old")));
        QCOMPARE(table()->item(table()->currentRow(), 0)->data(Qt::UserRole).toString(),
                 s("pv-compatible-old"));
        auto *loader = view->findChild<QComboBox *>(s("project-loader"));
        loader->setCurrentIndex(loader->findData(s("fabric")));
        QCOMPARE(choices()->count(), 2);
        QCOMPARE(choices()->currentData().toString(), s("pv-compatible-old"));
        auto *game = view->findChild<QComboBox *>(s("project-game"));
        game->setCurrentIndex(game->findData(s("1.21.1")));
        QCOMPARE(choices()->currentData().toString(), s("pv-compatible-old"));
        const auto newerRect = table()->visualItemRect(table()->item(row(s("pv-new")), 0));
        QTest::mouseClick(table()->viewport(), Qt::LeftButton, Qt::NoModifier, newerRect.center());
        QCOMPARE(choices()->currentData().toString(), s("pv-new"));
        QTest::keyClick(table(), Qt::Key_Down);
        QCOMPARE(choices()->currentData().toString(), s("pv-compatible-old"));
        game->setCurrentIndex(game->findData(s("1.20.1")));
        QCOMPARE(choices()->count(), 0);
        QVERIFY(!install()->isEnabled());
        QVERIFY(view->findChild<QWidget *>(s("project-versions-empty"))->isVisible());
        game->setCurrentIndex(game->findData(s("1.21.1")));
        QVERIFY(install()->isEnabled());
    }

    void targetBuildControlsCompatibilityAndDestination() {
        open(s("project-test-mod"), s("mod"));
        QTRY_COMPARE(choices()->count(), 2);
        QTRY_VERIFY(install()->isEnabled());
        auto *target = view->findChild<QComboBox *>(s("project-target"));
        auto *game = view->findChild<QComboBox *>(s("project-game"));
        auto *loader = view->findChild<QComboBox *>(s("project-loader"));
        QCOMPARE(game->currentData().toString(), s("1.21.1"));
        QCOMPARE(loader->currentData().toString(), s("fabric"));
        target->setCurrentIndex(target->findData(s("forge-build")));
        QCOMPARE(game->currentData().toString(), s("1.20.1"));
        QCOMPARE(loader->currentData().toString(), s("forge"));
        QCOMPARE(choices()->count(), 1);
        QCOMPARE(choices()->currentData().toString(), s("pv-old"));
        versionsTab();
        QSignalSpy requested(view, &ProjectView::installRequested);
        QTest::mouseClick(rowButton(s("pv-old")), Qt::LeftButton);
        QCOMPARE(requested.count(), 1);
        const auto request = requested.takeFirst();
        QCOMPARE(request[1].toString(), s("pv-old"));
        QCOMPARE(request[2].toString(), s("forge-build"));
        QVERIFY(!request[3].toBool());
        view->setInstalling(false);
        target->setCurrentIndex(target->findData(s("older-build")));
        QCOMPARE(game->currentData().toString(), s("1.19.4"));
        QCOMPARE(loader->currentData().toString(), s("vanilla"));
        QCOMPARE(choices()->count(), 0);
        QVERIFY(!install()->isEnabled());
    }

    void rebuildingRowsUsesTheCurrentInstallButton() {
        open();
        QTRY_COMPARE(choices()->count(), 3);
        QTRY_VERIFY(install()->isEnabled());
        versionsTab();
        QPointer<QPushButton> retired = rowButton(s("pv-new"));
        QVERIFY(retired);
        auto *loader = view->findChild<QComboBox *>(s("project-loader"));
        loader->setCurrentIndex(loader->findData(s("fabric")));
        QCOMPARE(choices()->count(), 2);
        auto *current = rowButton(s("pv-new"));
        QVERIFY(current);
        QVERIFY(current != retired.data());
        QCOMPARE(current->parentWidget(), table()->cellWidget(row(s("pv-new")), 1));
        QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
        QVERIFY(retired.isNull());
        QSignalSpy requested(view, &ProjectView::installRequested);
        QTest::mouseClick(current, Qt::LeftButton);
        QCOMPARE(requested.count(), 1);
        QCOMPARE(requested.takeFirst()[1].toString(), s("pv-new"));
    }

    void busyAndFailureRemainCoherent() {
        open();
        QTRY_COMPARE(choices()->count(), 3);
        QTRY_VERIFY(install()->isEnabled());
        choices()->setCurrentIndex(choices()->findData(s("pv-old")));
        view->setInstalling(true);
        auto *game = view->findChild<QComboBox *>(s("project-game"));
        QVERIFY(!game->isEnabled());
        game->setCurrentIndex(game->findData(s("1.20.1")));
        QCOMPARE(choices()->currentData().toString(), s("pv-old"));
        QVERIFY(!install()->isEnabled());
        QVERIFY(!rowButton(s("pv-old"))->isEnabled());
        view->setInstallationError(s("Test install failed; retry is available"));
        QVERIFY(install()->isEnabled());
        QVERIFY(rowButton(s("pv-old"))->isEnabled());
        QCOMPARE(choices()->currentData().toString(), s("pv-old"));
        QVERIFY(status()->text().contains(s("Test install failed")));
        view->setInstalling(false);
        QVERIFY(status()->text().contains(s("Test install failed")));
        QSignalSpy requested(view, &ProjectView::installRequested);
        QTest::mouseClick(install(), Qt::LeftButton);
        QCOMPARE(requested.count(), 1);
        QVERIFY(!status()->text().contains(s("Test install failed")));
        QVERIFY(!install()->isEnabled());
        view->setInstalling(false);
        QVERIFY(install()->isEnabled());
        QCOMPARE(choices()->currentData().toString(), s("pv-old"));
    }

    void externalBlockKeepsItsReasonAndPreservesTheLastError() {
        open();
        QTRY_COMPARE(choices()->count(), 3);
        QTRY_VERIFY(install()->isEnabled());
        choices()->setCurrentIndex(choices()->findData(s("pv-old")));
        const auto failure = s("Test previous installation failed");
        const auto running = s("Test Minecraft is running; stop it before installing");
        const auto offline = s("Test backend disconnected; restart the launcher");
        view->setInstallationError(failure);
        view->setActionBlockedReason(running);
        QVERIFY(!install()->isEnabled());
        QVERIFY(!rowButton(s("pv-old"))->isEnabled());
        QVERIFY(choices()->isEnabled());
        QVERIFY(view->findChild<QComboBox *>(s("project-game"))->isEnabled());
        QCOMPARE(status()->text(), running);
        QCOMPARE(install()->text(), QString::fromUtf8("Установить"));
        QSignalSpy requested(view, &ProjectView::installRequested);
        QTest::mouseClick(install(), Qt::LeftButton);
        QCOMPARE(requested.count(), 0);
        view->setInstalling(false);
        QCOMPARE(status()->text(), running);
        view->setActionBlockedReason({});
        QVERIFY(install()->isEnabled());
        QCOMPARE(choices()->currentData().toString(), s("pv-old"));
        QCOMPARE(status()->text(), failure);

        view->setActionBlockedReason(offline);
        open();
        QTRY_COMPARE(choices()->count(), 3);
        QTRY_VERIFY(choices()->isEnabled());
        QCOMPARE(status()->text(), offline);
        QVERIFY(!install()->isEnabled());
        view->setInstalling(true);
        QCOMPARE(status()->text(), offline);
        QCOMPARE(install()->text(), QString::fromUtf8("Установить"));
        view->setInstalling(false);
        QCOMPARE(status()->text(), offline);
        QVERIFY(!install()->isEnabled());
        view->setActionBlockedReason({});
        QVERIFY(install()->isEnabled());
        QVERIFY(!status()->text().contains(offline));
    }

    void emptyVersionsAndMissingTargetHaveAnExplanation() {
        open(s("project-test-empty"));
        versionsTab();
        auto *empty = view->findChild<QWidget *>(s("project-versions-empty"));
        auto *retry = view->findChild<QPushButton *>(s("project-retry"));
        QTRY_VERIFY(view->findChild<QComboBox *>(s("project-game"))->isEnabled());
        QCOMPARE(choices()->count(), 0);
        QVERIFY(!install()->isEnabled());
        QVERIFY(empty->isVisible());
        QVERIFY(!retry->isVisible());
        QVERIFY(!status()->text().isEmpty());
        snapshot(s("project-versions-empty"));
        view->open({{s("id"), s("project-test-mod")}, {s("project_type"), s("mod")}}, {}, {});
        versionsTab();
        QTRY_VERIFY(view->findChild<QComboBox *>(s("project-game"))->isEnabled());
        QVERIFY(!view->findChild<QComboBox *>(s("project-target"))->isEnabled());
        QCOMPARE(choices()->count(), 0);
        QVERIFY(!install()->isEnabled());
        QVERIFY(empty->isVisible());
        QVERIFY(status()->text().contains(QString::fromUtf8("сборку")));
    }

    void failedVersionsCanBeReloaded() {
        open(s("project-test-error"));
        versionsTab();
        auto *retry = view->findChild<QPushButton *>(s("project-retry"));
        QTRY_VERIFY(retry->isVisible());
        QVERIFY(!install()->isEnabled());
        QVERIFY(status()->text().contains(s("Test versions unavailable")));
        snapshot(s("project-versions-error"));
        QTest::mouseClick(retry, Qt::LeftButton);
        QTRY_COMPARE(choices()->count(), 3);
        QTRY_VERIFY(install()->isEnabled());
        QVERIFY(!retry->isVisible());
        QVERIFY(!status()->text().contains(s("Test versions unavailable")));
    }

    void metadataFailureCannotInstallOrLeakIntoAnotherProject() {
        open(s("project-test-metadata-error"));
        versionsTab();
        QTRY_VERIFY(status()->text().contains(s("Test project unavailable")));
        QVERIFY(!install()->isEnabled());
        QVERIFY(view->findChild<QPushButton *>(s("project-retry"))->isVisible());
        open(s("project-test-metadata-error"));
        open();
        QTRY_COMPARE(choices()->count(), 3);
        QTRY_VERIFY(install()->isEnabled());
        QVERIFY(!status()->text().contains(s("Test project unavailable")));
    }

    void narrowLayoutKeepsEveryActionReachable() {
        open();
        QTRY_COMPARE(choices()->count(), 3);
        QTRY_VERIFY(install()->isEnabled());
        versionsTab();
        snapshot(s("project-versions"));
        for (const auto &size : {QSize(900, 620), QSize(640, 720), QSize(1180, 650)}) {
            view->resize(size);
            QTest::qWait(80);
            QCOMPARE(view->size(), size);
            for (auto *area : view->findChildren<QScrollArea *>())
                if (area->isVisible())
                    QVERIFY2(area->widget()->width() <= area->viewport()->width(),
                             "Installation controls must not overflow horizontally");
            auto *action = rowButton(s("pv-new"));
            QVERIFY(action->isVisible());
            const auto point = action->mapTo(view, QPoint());
            QVERIFY(point.x() >= 0);
            QVERIFY(point.x() + action->width() <= view->width());
            QVERIFY(action->width() >= 90);
            QCOMPARE(table()->horizontalScrollBar()->maximum(), 0);
            if (size.width() == 640)
                snapshot(s("project-versions-narrow"));
        }
    }

    void cleanupTestCase() {
        backend->shutdown();
    }
};

QTEST_MAIN(ProjectViewTest)
#include "projectview-test.moc"
