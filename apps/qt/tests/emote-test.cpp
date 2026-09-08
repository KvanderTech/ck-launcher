#include "emote.h"
#include "skinview.h"
#include <QtTest>
#include <cmath>

namespace {
constexpr float pi = 3.14159265358979323846f;
EmoteClip json(const QByteArray &source) {
    return EmoteClip(QJsonDocument::fromJson(source).object());
}
bool vectorsNear(const QVector3D &a, const QVector3D &b, float tolerance = .001f) {
    return (a - b).length() < tolerance;
}
QImage testSkin() {
    QImage image(qEnvironmentVariable("CK_TEST_SKIN"));
    if (!image.isNull())
        return image;
    image = QImage(64, 64, QImage::Format_ARGB32);
    image.fill(Qt::transparent);
    QPainter p(&image);
    const QVector<QRect> parts{{0, 0, 32, 16},   {16, 16, 24, 16}, {40, 16, 16, 16},
                               {32, 48, 16, 16}, {0, 16, 16, 16},  {16, 48, 16, 16}};
    for (int i = 0; i < parts.size(); ++i) {
        const auto r = parts[i];
        for (int y = r.top(); y <= r.bottom(); ++y)
            for (int x = r.left(); x <= r.right(); ++x)
                p.fillRect(x, y, 1, 1,
                           QColor::fromHsv((i * 55) % 360, 170, ((x + y) % 2) ? 230 : 180));
    }
    p.fillRect(8, 8, 8, 8, QColor(236, 190, 158));
    p.fillRect(8, 8, 8, 2, QColor(20, 28, 32));
    p.fillRect(9, 11, 2, 2, Qt::white);
    p.fillRect(13, 11, 2, 2, Qt::white);
    p.fillRect(10, 11, 1, 2, Qt::black);
    p.fillRect(13, 11, 1, 2, Qt::black);
    return image;
}
} // namespace

class EmoteTest final : public QObject {
    Q_OBJECT
  private slots:
    void initTestCase() {
        for (const auto &font : {QStringLiteral("segoeui.ttf"), QStringLiteral("segoeuib.ttf")})
            QFontDatabase::addApplicationFont(qEnvironmentVariable("WINDIR") +
                                              QStringLiteral("/Fonts/") + font);
        qApp->setFont(QFont(QStringLiteral("Segoe UI"), 10));
    }
    void legacyTorsoIsRootAndV3TorsoIsChest() {
        const auto legacy = json(R"({"emote":{"endTick":20,"degrees":false,"moves":[
          {"tick":0,"torso":{"y":1,"pitch":0.5}}]}})");
        QCOMPARE(legacy.sample(QStringLiteral("body"), 10).position.y(), 1.f);
        QCOMPARE(legacy.sample(QStringLiteral("body"), 10).rotation.x(), .5f);
        QCOMPARE(legacy.sample(QStringLiteral("torso"), 10).position, QVector3D());
        const auto v3 = json(R"({"version":3,"emote":{"endTick":20,"degrees":false,"moves":[
          {"tick":0,"torso":{"y":1,"pitch":0.5}}]}})");
        QCOMPARE(v3.sample(QStringLiteral("body"), 10).position, QVector3D());
        QCOMPARE(v3.sample(QStringLiteral("torso"), 10).position.y(), 1.f);
    }
    void rootUnitsPivotAndCoordinateSystems() {
        EmotePart p;
        p.position = {1, 1, 1};
        QVERIFY(vectorsNear(EmoteClip::bodyTransform(p).map(QVector3D()), {-16, 16, -16}));
        p.position = {};
        p.rotation = {-pi / 2, 0, 0};
        const auto root = EmoteClip::bodyTransform(p);
        QVERIFY(vectorsNear(root.map({0, 11.2f, 0}), {0, 11.2f, 0}));
        QVERIFY(vectorsNear(root.map({0, 24, 0}), {0, 11.2f, 12.8f}));
        const auto arm = EmoteClip::modelTransform(p);
        QVERIFY(vectorsNear(arm.map({0, -6, 0}), {0, 24, 6}));
    }
    void easingDegreesLoopAndStop() {
        const auto make = [](bool before) {
            auto object = QJsonDocument::fromJson(R"({"version":3,"emote":{"endTick":10,
              "moves":[{"tick":0,"easing":"EASEINQUAD","head":{"x":0}},
              {"tick":10,"easing":"EASEOUTCUBIC","head":{"x":10,"pitch":180}}]}})")
                              .object();
            auto data = object.value(QStringLiteral("emote")).toObject();
            data.insert(QStringLiteral("easeBeforeKeyframe"), before);
            object.insert(QStringLiteral("emote"), data);
            return EmoteClip(object);
        };
        QVERIFY(std::abs(make(false).sample(QStringLiteral("head"), 5).position.x() - 2.5f) < .001);
        QVERIFY(std::abs(make(true).sample(QStringLiteral("head"), 5).position.x() - 8.75f) < .001);
        QVERIFY(std::abs(make(false).sample(QStringLiteral("head"), 10).rotation.x() - pi) < .001);
        const auto loop = json(R"({"emote":{"endTick":20,"isLoop":"true","returnTick":10,
          "moves":[{"tick":10,"head":{"x":5}},{"tick":20,"head":{"x":7}}]}})");
        QVERIFY(std::abs(loop.sample(QStringLiteral("head"), 20.5).position.x() - 6) < .001);
        QVERIFY(vectorsNear(loop.sample(QStringLiteral("head"), 14).position,
                            loop.sample(QStringLiteral("head"), 25).position));
        const auto stop = json(R"({"emote":{"endTick":10,"stopTick":20,"moves":[
          {"tick":10,"head":{"x":10}}]}})");
        QCOMPARE(stop.sample(QStringLiteral("head"), 15).position.x(), 5.f);
        QCOMPARE(stop.sample(QStringLiteral("head"), 20).position.x(), 0.f);
    }
    void jointPlaneIsClosedAndEndsAreRigid() {
        for (const bool upper : {false, true})
            for (const float centerX : {-1.f, -.5f, 0.f})
                for (const float axis : {0.f, .8f, pi / 2})
                    for (const float angle : {-.9f, .9f, 1.8f}) {
                        const QVector3D center(centerX, -4, 0);
                        const QVector3D direction(0, upper ? 1.f : -1.f, 0);
                        const auto bend = [&](QVector3D v) {
                            return EmoteClip::bendVertex(v, center, 6, angle, axis, upper);
                        };
                        const auto rotation = EmoteClip::bendTransform(center, angle, axis, upper);
                        for (const float x : {-2.f, 2.f})
                            for (const float z : {-2.f, 2.f}) {
                                const auto atJoint = center + QVector3D(x, 0, z);
                                QVERIFY(vectorsNear(bend(atJoint + direction * .00001f),
                                                    bend(atJoint - direction * .00001f)));
                                const auto fixed = atJoint - direction * 6;
                                const auto tip = atJoint + direction * 6;
                                QVERIFY(vectorsNear(bend(fixed), fixed));
                                QVERIFY(vectorsNear(bend(tip), rotation.map(tip)));
                            }
                    }
        QVERIFY(
            vectorsNear(EmoteClip::bendVertex({1, -4, 2}, {-1, -4, 0}, 6, pi / 2, 0), {1, -6, 2}));
    }
    void upperBodyAndFadeDoNotTwistLimbs() {
        const auto upper = EmoteClip::bendTransform({0, 18, 0}, .8f, .6f, true);
        const QVector3D shoulder(-5, 22, 0), hand(-5, 10, 0);
        QVERIFY(std::abs((upper.map(shoulder) - upper.map(hand)).length() - 12) < .001);
        EmotePart p;
        p.rotation = {0, 0, 3 * pi / 2};
        const auto halfway = EmoteClip::modelTransform(p, .5);
        const auto direction = halfway.mapVector({1, 0, 0});
        QVERIFY(direction.x() > .70f && direction.y() > .70f);
    }
    void realClipsRenderAllPhases_data() {
        QTest::addColumn<bool>("slim");
        QTest::addColumn<bool>("legacy");
        QTest::newRow("classic") << false << false;
        QTest::newRow("slim") << true << false;
        QTest::newRow("legacy") << false << true;
    }
    void completePlaybackDoesNotClip_data() {
        QTest::addColumn<bool>("slim");
        QTest::addColumn<bool>("legacy");
        QTest::addColumn<int>("turn");
        QTest::newRow("front") << false << false << 0;
        QTest::newRow("side") << false << false << 150;
        QTest::newRow("back") << false << false << 300;
        QTest::newRow("other-side") << false << false << 450;
        QTest::newRow("slim") << true << false << 0;
        QTest::newRow("legacy") << false << true << 0;
    }
    void completePlaybackDoesNotClip() {
        QFETCH(bool, slim);
        QFETCH(bool, legacy);
        QFETCH(int, turn);
        SkinView view;
        view.resize(220, 320);
        auto texture = testSkin();
        if (legacy)
            texture = texture.copy(0, 0, texture.width(), texture.width() / 2);
        view.setSkin(texture, slim);
        view.setAnimated(true);
        QMouseEvent press(QEvent::MouseButtonPress, QPointF(100, 120), Qt::LeftButton,
                          Qt::LeftButton, Qt::NoModifier);
        QMouseEvent move(QEvent::MouseMove, QPointF(100 + turn, 120), Qt::NoButton, Qt::LeftButton,
                         Qt::NoModifier);
        QApplication::sendEvent(&view, &press);
        QApplication::sendEvent(&view, &move);
        for (int tick = 0; tick <= 640; tick += 2) {
            view.setPoseTime(tick / 20.0);
            QImage image(view.size(), QImage::Format_ARGB32);
            image.fill(Qt::transparent);
            view.render(&image, QPoint(), QRegion(), QWidget::DrawChildren);
            bool clipped = false;
            for (int x = 0; x < image.width(); ++x)
                clipped |= qAlpha(image.pixel(x, 0)) != 0 ||
                           qAlpha(image.pixel(x, image.height() - 1)) != 0;
            for (int y = 0; y < image.height(); ++y)
                clipped |= qAlpha(image.pixel(0, y)) != 0 ||
                           qAlpha(image.pixel(image.width() - 1, y)) != 0;
            if (clipped) {
                const auto output = qEnvironmentVariable("CK_UI_SCREENSHOTS");
                if (!output.isEmpty())
                    image.save(output + QStringLiteral("/clipped-%1-%2.png")
                                            .arg(QString::fromLatin1(QTest::currentDataTag()))
                                            .arg(tick));
            }
            QVERIFY2(!clipped, qPrintable(QStringLiteral("Pose clipped at tick %1").arg(tick)));
        }
    }
    void realClipsRenderAllPhases() {
        QFETCH(bool, slim);
        QFETCH(bool, legacy);
        auto texture = testSkin();
        if (legacy)
            texture = texture.copy(0, 0, texture.width(), texture.width() / 2);
        SkinView view;
        view.resize(220, 320);
        view.setSkin(texture, slim);
        view.setAnimated(true);
        QString output = qEnvironmentVariable("CK_UI_SCREENSHOTS");
        if (output.isEmpty())
            output = QDir::currentPath() + QStringLiteral("/ui-screenshots");
        QVERIFY(QDir().mkpath(output));
        const QStringList names{QStringLiteral("yes"), QStringLiteral("wave"),
                                QStringLiteral("bow"), QStringLiteral("extend-arms")};
        QImage sheet(220 * 6, 350 * 4, QImage::Format_ARGB32);
        sheet.fill(QColor(8, 26, 44));
        QPainter painter(&sheet);
        painter.setPen(Qt::white);
        for (int c = 0; c < names.size(); ++c) {
            const auto clip = EmoteClip::load(QStringLiteral(":/assets/emotes/") + names[c] +
                                              QStringLiteral(".json"));
            QVERIFY(clip.valid());
            for (int phase = 0; phase < 6; ++phase) {
                const double local =
                    phase == 5 ? clip.duration() + .2 : clip.duration() * phase / 4;
                view.setPoseTime(c * 8 + 2.5 + local);
                QImage image(view.size(), QImage::Format_ARGB32);
                image.fill(Qt::transparent);
                view.render(&image, QPoint(), QRegion(), QWidget::DrawChildren);
                painter.drawImage(phase * 220, c * 350 + 30, image);
                painter.drawText(QRect(phase * 220, c * 350, 220, 30), Qt::AlignCenter,
                                 names[c] + QStringLiteral("  ") + QString::number(local, 'f', 2));
            }
        }
        painter.end();
        QVERIFY(sheet.save(output + QStringLiteral("/emotes-") +
                           QString::fromLatin1(QTest::currentDataTag()) + QStringLiteral(".png")));
    }
};
QTEST_MAIN(EmoteTest)
#include "emote-test.moc"
