"""Recorded current-release disagreement controls; no native agent or network."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import unittest
from unittest import mock

ROOT=Path(__file__).resolve().parents[1]
FIXTURE=ROOT/'tests/fixtures/kimi-current-release'
DATA=json.loads((FIXTURE/'current.json').read_text())
spec=importlib.util.spec_from_file_location('kimi_current',ROOT/'scripts/ci/kimi-native.py')
ci=importlib.util.module_from_spec(spec);spec.loader.exec_module(ci)


class KimiCurrentRelease(unittest.TestCase):
    def sources(self, versions=('2.5.3','2.6.2','2.6.2')):
        data=copy.deepcopy(DATA)
        data['standalone']['body']=versions[0]+'\n'
        data['npm']['response']['version']=versions[1]
        data['github']['response']['tag_name']='@moonshot-ai/kimi-code@'+versions[2]
        data['github']['response']['html_url']='https://github.com/MoonshotAI/kimi-code/releases/tag/%40moonshot-ai/kimi-code%40'+versions[2]
        return data

    def resolve(self, data=None, failed=None, raw=None):
        data=data or self.sources();calls=[]
        def read(url,limit,method='GET',**kwargs):
            calls.append((url,method))
            for source in ('standalone','npm','github'):
                if url==data[source]['url']:
                    if source in ('npm','github'):
                        self.assertEqual(kwargs,{'policy':'no-redirect'})
                        self.assertEqual(limit,65536 if source=='npm' else 262144)
                    if failed==source:raise ci.discovery.DiscoveryUnavailable('recorded source unavailable')
                    body=(data[source]['body'].encode() if source=='standalone' else json.dumps(data[source]['response']).encode())
                    if raw and source in raw:body=raw[source]
                    return body,{'url':url,'final_url':url,'size':len(body)}
            prefix='https://code.kimi.com/kimi-code/binaries/'
            self.assertTrue(url.startswith(prefix),url)
            version=url[len(prefix):].split('/')[0]
            manifest=json.loads((FIXTURE/'manifest-2.6.2.json').read_text())
            if url.endswith('/manifest.json'):
                if version=='2.6.2':body=(FIXTURE/'manifest-2.6.2.json').read_bytes()
                else:
                    manifest.update(version=version,tag='@moonshot-ai/kimi-code@'+version)
                    body=json.dumps(manifest).encode()
                return body,{'url':url,'final_url':url,'size':len(body)}
            self.assertEqual((method,url.rsplit('/',1)[1]),('HEAD','kimi-code-linux-x64'))
            return b'',{'url':url,'final_url':url,'size':data['linux_image']['size']}
        with mock.patch.object(ci,'metadata',side_effect=read):result=ci.resolve_latest('linux-x64')
        return result,calls

    def test_recorded_standalone_lag_selects_the_corroborated_newest_release(self):
        (record,manifest,evidence),calls=self.resolve()
        self.assertEqual(record['version'],'2.6.2')
        self.assertEqual((record['image_size'],record['image_sha256']),(DATA['linux_image']['size'],DATA['linux_image']['sha256']))
        self.assertEqual(record['manifest_sha256'],hashlib.sha256((FIXTURE/'manifest-2.6.2.json').read_bytes()).hexdigest())
        self.assertEqual(manifest,(FIXTURE/'manifest-2.6.2.json').read_bytes())
        self.assertEqual([url for url,method in calls[:3]],[DATA[key]['url'] for key in ('standalone','npm','github')])
        self.assertTrue(all('/2.6.2/' in url for url,method in calls[3:]))
        self.assertEqual(len(calls),5)
        decision=evidence['selection']
        self.assertEqual(decision['version'],'2.6.2')
        self.assertEqual(decision['agreeing_sources'],['npm','github'])
        self.assertEqual({key:row['version'] for key,row in decision['sources'].items()},dict(standalone='2.5.3',npm='2.6.2',github='2.6.2'))

    def test_all_sources_agree_and_are_each_read_once(self):
        (record,_,_),calls=self.resolve(self.sources(('2.6.2',)*3))
        self.assertEqual(record['version'],'2.6.2');self.assertEqual(len(calls),5)

    def test_numeric_version_order_and_either_independent_pair_can_agree(self):
        for versions in [('2.10.0','2.9.9','2.10.0'),('2.10.0','2.10.0','2.9.9')]:
            with self.subTest(versions=versions):
                (record,_,_),calls=self.resolve(self.sources(versions));self.assertEqual(record['version'],'2.10.0');self.assertEqual(len(calls),5)

    def test_three_different_versions_fail_closed(self):
        with self.assertRaises(ValueError):self.resolve(self.sources(('2.5.3','2.6.2','2.7.0')))

    def test_a_higher_unconfirmed_outlier_never_falls_back_to_an_older_pair(self):
        for versions in [('2.6.2','2.5.3','2.5.3'),('2.5.3','2.6.2','2.5.3'),('2.5.3','2.5.3','2.6.2')]:
            with self.subTest(versions=versions),self.assertRaises(ValueError):self.resolve(self.sources(versions))

    def test_any_official_source_failure_blocks_selection(self):
        for source in ('standalone','npm','github'):
            with self.subTest(source=source),self.assertRaises(ci.discovery.DiscoveryUnavailable):self.resolve(failed=source)

    def test_invalid_or_duplicate_source_metadata_cannot_be_ignored(self):
        variants=[('npm',b'{"name":"@moonshot-ai/kimi-code","version":"2.6.2","version":"2.5.3"}'),('github',b'{"tag_name":"x","tag_name":"y"}'),('npm',b'not JSON'),('github',b'[]')]
        for source,body in variants:
            with self.subTest(source=source,body=body),self.assertRaises((ValueError,TypeError)):self.resolve(raw={source:body})

    def test_npm_must_name_the_official_package(self):
        data=self.sources();data['npm']['response']['name']='@other/kimi-code'
        with self.assertRaises(ValueError):self.resolve(data)

    def test_github_must_be_a_stable_official_release(self):
        for key,value in [('draft',True),('prerelease',True),('draft',None),('prerelease',0),('html_url',None),('html_url',42),('tag_name','@other/code@2.6.2'),('html_url','https://example.invalid/release')]:
            data=self.sources();data['github']['response'][key]=value
            with self.subTest(key=key,value=value),self.assertRaises(ValueError):self.resolve(data)

    def test_a_different_package_tag_on_the_official_repo_cannot_endorse_kimi(self):
        data=self.sources();data['github']['response'].update(tag_name='@other/code@2.6.2',html_url='https://github.com/MoonshotAI/kimi-code/releases/tag/%40other/code%402.6.2')
        with self.assertRaises(ValueError):self.resolve(data)

    def test_each_source_version_must_be_one_bounded_stable_version(self):
        for source in ('standalone','npm','github'):
            for version in ['2.6.2-beta','02.6.2','2.6.2\n2.5.3','9999999.0.0']:
                versions=['2.6.2']*3;versions[('standalone','npm','github').index(source)]=version
                with self.subTest(source=source,version=version),self.assertRaises(ValueError):self.resolve(self.sources(tuple(versions)))


if __name__=='__main__':unittest.main()
